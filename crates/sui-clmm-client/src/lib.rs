//! Multi-venue read API for agents and the TUI.
//!
//! Primary path: **wallet positions**. Pool prefetch + [`router`] stay available
//! for later swap flows but are not required by the TUI today.

#![forbid(unsafe_code)]

use anyhow::Result;
use router::{Router, RouterUniverse, SwapRoute};
use sui_clmm_manager::graphql::{self, GraphQl, GraphQlMeter};
use sui_clmm_manager::store::Store;
use sui_clmm_manager::{
    PoolDetail, PoolRow, PositionDetail, PositionRow, PrefetchStats, Venue, VenueFilter,
};

pub use router::{RouteHop, Router as SwapRouter, RouterUniverse as SwapUniverse};
pub use sui_clmm_manager::{
    DEFAULT_GRAPHQL_URL, GraphQlCall, PositionDetail as Detail, PositionRow as Row, Venue as Ven,
    VenueFilter as Filter,
};

#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    /// Used only by [`Client::prefetch_pools`] (deferred TUI feature).
    pub pool_limit: usize,
}

impl ClientConfig {
    pub fn with_pool_limit(mut self, limit: usize) -> Self {
        self.pool_limit = limit.clamp(1, 500);
        self
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    pub config: ClientConfig,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            config: ClientConfig { pool_limit: 250 },
        }
    }
}

impl Client {
    pub fn new(pool_limit: usize) -> Self {
        Self {
            config: ClientConfig {
                pool_limit: pool_limit.clamp(1, 500),
            },
        }
    }

    pub fn with_config(config: ClientConfig) -> Self {
        Self { config }
    }

    pub fn connect<'a>(url: &str, meter: &'a GraphQlMeter) -> Result<GraphQl<'a>> {
        graphql::GraphQl::connect(url, meter)
    }

    /// Soft-fail merge of venue position lists, then optional venue filter.
    pub fn list_positions(
        &self,
        gql: &GraphQl<'_>,
        store: Option<&Store>,
        owner: &str,
        filter: VenueFilter,
    ) -> Result<Vec<PositionRow>> {
        let mut rows = Vec::new();
        let mut push = |venue: Venue, res: Result<Vec<PositionRow>>| {
            if !filter.matches(venue) {
                return;
            }
            if let Ok(mut v) = res {
                rows.append(&mut v);
            }
        };
        push(Venue::Bluefin, bluefin::list_positions(gql, store, owner));
        push(Venue::Momentum, momentum::list_positions(gql, store, owner));
        push(Venue::Cetus, cetus::list_positions(gql, store, owner));
        push(Venue::Turbos, turbos::list_positions(gql, store, owner));
        push(Venue::Deepbook, deepbook::list_positions(gql, store, owner));
        rows.sort_by(|a, b| {
            a.venue
                .cmp(&b.venue)
                .then(b.liquidity.cmp(&a.liquidity))
        });
        Ok(rows)
    }

    pub fn position_detail(
        &self,
        gql: &GraphQl<'_>,
        store: Option<&Store>,
        row: &PositionRow,
    ) -> Result<PositionDetail> {
        let mut detail = match row.venue {
            Venue::Bluefin => bluefin::fetch_position_detail(gql, store, row)?,
            Venue::Momentum => momentum::fetch_position_detail(gql, store, row)?,
            Venue::Cetus => cetus::fetch_position_detail(gql, store, row)?,
            Venue::Turbos => turbos::fetch_position_detail(gql, store, row)?,
            Venue::Deepbook => deepbook::fetch_position_detail(gql, store, row)?,
        };
        let price_mode = store
            .and_then(|s| s.load_settings().ok())
            .map(|s| sui_clmm_manager::PriceMode::from_setting(&s.price_mode))
            .unwrap_or_default();
        detail.attach_usd(price_mode);
        if let Some(store) = store {
            let _ = store.record_position_snapshot(&detail);
        }
        Ok(detail)
    }

    pub fn position_history(
        &self,
        store: &Store,
        object_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<sui_clmm_manager::PositionSnapshotRow>> {
        store.list_position_snapshots(object_id, limit)
    }

    // ── pool prefetch / router (kept for later; unused by TUI for now) ─────

    pub fn prefetch_pools(
        &self,
        gql: &GraphQl<'_>,
        store: &Store,
        filter: VenueFilter,
    ) -> Result<PrefetchStats> {
        let before = gql.meter.graphql_count();
        let mut merged = Vec::new();
        let mut venues = Vec::new();
        let limit = self.config.pool_limit;

        let try_venue = |venue: Venue,
                         fetch: fn(&GraphQl<'_>, Option<&Store>, usize) -> Result<Vec<PoolRow>>,
                         merged: &mut Vec<PoolRow>,
                         venues: &mut Vec<(Venue, usize)>| {
            if !filter.matches(venue) {
                return;
            }
            match fetch(gql, Some(store), limit) {
                Ok(rows) => {
                    venues.push((venue, rows.len()));
                    merged.extend(rows);
                }
                Err(_) => venues.push((venue, 0)),
            }
        };

        try_venue(Venue::Bluefin, bluefin::list_top_pools, &mut merged, &mut venues);
        try_venue(Venue::Momentum, momentum::list_top_pools, &mut merged, &mut venues);
        try_venue(Venue::Cetus, cetus::list_top_pools, &mut merged, &mut venues);
        try_venue(Venue::Turbos, turbos::list_top_pools, &mut merged, &mut venues);
        try_venue(Venue::Deepbook, deepbook::list_top_pools, &mut merged, &mut venues);

        merged.sort_by(|a, b| b.liquidity.cmp(&a.liquidity));
        if merged.len() > limit {
            merged.truncate(limit);
        }
        for (i, row) in merged.iter_mut().enumerate() {
            row.rank = (i + 1) as u32;
        }
        let stored = merged.len();
        store.replace_pool_snapshots(&merged)?;
        Ok(PrefetchStats {
            fetched: stored,
            stored,
            venues,
            graphql_calls: gql.meter.graphql_count().saturating_sub(before),
        })
    }

    pub fn list_pools(
        &self,
        store: &Store,
        filter: VenueFilter,
        symbol_query: Option<&str>,
    ) -> Result<Vec<PoolRow>> {
        let venue = match filter {
            VenueFilter::All => None,
            VenueFilter::Only(v) => Some(v),
        };
        let mut rows = store.list_pool_snapshots(venue)?;
        if let Some(q) = symbol_query.map(str::trim).filter(|s| !s.is_empty()) {
            let q = q.to_ascii_lowercase();
            rows.retain(|r| {
                r.pair_label().to_ascii_lowercase().contains(&q)
                    || r.name.to_ascii_lowercase().contains(&q)
                    || r.object_id.to_ascii_lowercase().contains(&q)
            });
        }
        Ok(rows)
    }

    pub fn pool_detail(
        &self,
        gql: &GraphQl<'_>,
        store: Option<&Store>,
        row: &PoolRow,
    ) -> Result<PoolDetail> {
        match row.venue {
            Venue::Bluefin => bluefin::fetch_pool_detail(gql, store, &row.object_id),
            Venue::Momentum => momentum::fetch_pool_detail(gql, store, &row.object_id),
            Venue::Cetus => cetus::fetch_pool_detail(gql, store, &row.object_id),
            Venue::Turbos => turbos::fetch_pool_detail(gql, store, &row.object_id),
            Venue::Deepbook => deepbook::fetch_pool_detail(gql, store, &row.object_id),
        }
    }

    pub fn swap_router(&self, store: &Store, filter: VenueFilter) -> Result<Router> {
        Ok(Router::new(RouterUniverse::from_store(store, filter)?))
    }

    pub fn plan_swap(
        &self,
        store: &Store,
        coin_in: &str,
        coin_out: &str,
        amount_in: u128,
    ) -> Result<SwapRoute> {
        self.swap_router(store, VenueFilter::All)?
            .plan_swap(coin_in, coin_out, amount_in)
    }
}
