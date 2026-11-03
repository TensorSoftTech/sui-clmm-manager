//! Multi-venue swap router over prefetched pools.
//!
//! Reads pool snapshots from [`sui_clmm_manager::Store`] (top-N prefetch) and
//! plans routes across Bluefin / Momentum / Cetus / Turbos / DeepBook.
//!
//! v0 is a **quote/plan stub**: path finding and PTB assembly land later
//! (likely with a Move helper for atomic multi-hop). DeepBook is the CLOB
//! leg; CLMM venues supply concentrated-liquidity hops.

#![forbid(unsafe_code)]

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sui_clmm_manager::{PoolRow, Store, Venue, VenueFilter};

/// One hop in a planned swap route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteHop {
    pub venue: String,
    pub pool_id: String,
    pub pair: String,
    pub a_to_b: bool,
    pub fee_rate: u32,
}

/// Planned route (not executed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapRoute {
    pub coin_in: String,
    pub coin_out: String,
    pub amount_in: u128,
    /// Estimated out — 0 until quote math lands.
    pub amount_out_est: u128,
    pub hops: Vec<RouteHop>,
    pub note: String,
}

/// Candidate pools the router may hop through (from prefetch cache).
#[derive(Debug, Clone)]
pub struct RouterUniverse {
    pub pools: Vec<PoolRow>,
}

impl RouterUniverse {
    /// Load ranked pool snapshots; optional venue filter.
    pub fn from_store(store: &Store, filter: VenueFilter) -> Result<Self> {
        let venue = match filter {
            VenueFilter::All => None,
            VenueFilter::Only(v) => Some(v),
        };
        let mut pools = store.list_pool_snapshots(venue)?;
        if filter != VenueFilter::All {
            pools.retain(|p| filter.matches(p.venue));
        }
        Ok(Self { pools })
    }

    pub fn pools_for_pair(&self, coin_a: &str, coin_b: &str) -> Vec<&PoolRow> {
        self.pools
            .iter()
            .filter(|p| {
                (types_eq(&p.coin_type_a, coin_a) && types_eq(&p.coin_type_b, coin_b))
                    || (types_eq(&p.coin_type_a, coin_b) && types_eq(&p.coin_type_b, coin_a))
            })
            .collect()
    }

    pub fn by_venue(&self, venue: Venue) -> impl Iterator<Item = &PoolRow> {
        self.pools.iter().filter(move |p| p.venue == venue)
    }
}

/// Swap router over a fixed universe of prefetched pools.
#[derive(Debug, Clone)]
pub struct Router {
    pub universe: RouterUniverse,
    /// Max hops for multi-pool paths (Move helper may enforce the same).
    pub max_hops: usize,
}

impl Router {
    pub fn new(universe: RouterUniverse) -> Self {
        Self {
            universe,
            max_hops: 3,
        }
    }

    pub fn with_max_hops(mut self, max_hops: usize) -> Self {
        self.max_hops = max_hops.max(1);
        self
    }

    /// Find direct (1-hop) pool candidates sorted by liquidity desc.
    pub fn direct_pools(&self, coin_in: &str, coin_out: &str) -> Vec<&PoolRow> {
        let mut hits = self.universe.pools_for_pair(coin_in, coin_out);
        hits.sort_by(|a, b| b.liquidity.cmp(&a.liquidity));
        hits
    }

    /// Plan a swap. v0: pick the highest-liquidity direct pool; no amount math yet.
    pub fn plan_swap(
        &self,
        coin_in: &str,
        coin_out: &str,
        amount_in: u128,
    ) -> Result<SwapRoute> {
        if types_eq(coin_in, coin_out) {
            bail!("coin_in == coin_out");
        }
        if amount_in == 0 {
            bail!("amount_in is zero");
        }

        let direct = self.direct_pools(coin_in, coin_out);
        if let Some(pool) = direct.first() {
            let a_to_b = types_eq(&pool.coin_type_a, coin_in);
            return Ok(SwapRoute {
                coin_in: coin_in.to_string(),
                coin_out: coin_out.to_string(),
                amount_in,
                amount_out_est: 0,
                hops: vec![RouteHop {
                    venue: pool.venue.short().to_string(),
                    pool_id: pool.object_id.clone(),
                    pair: pool.pair_label(),
                    a_to_b,
                    fee_rate: pool.fee_rate,
                }],
                note: "direct hop · quote math + PTB assemble deferred".into(),
            });
        }

        // Multi-hop placeholder: surface that no direct pool exists.
        // Future: BFS over `universe.pools` up to `max_hops`, then DeepBook legs.
        bail!(
            "no direct pool for {} → {} in prefetch universe ({} pools); multi-hop not wired yet",
            short_type(coin_in),
            short_type(coin_out),
            self.universe.pools.len()
        );
    }

    /// Status string for TUI / help.
    pub fn status(&self) -> String {
        format!(
            "router · {} pools cached · max_hops={} · quotes/PTB stub",
            self.universe.pools.len(),
            self.max_hops
        )
    }
}

fn types_eq(a: &str, b: &str) -> bool {
    normalize_type(a) == normalize_type(b)
}

fn normalize_type(t: &str) -> String {
    let t = t.trim();
    // Collapse leading zeros in address hex for loose compare.
    if let Some((addr, rest)) = t.split_once("::") {
        let addr = addr.trim_start_matches("0x");
        let addr = addr.trim_start_matches('0');
        format!("0x{addr}::{rest}")
    } else {
        t.to_string()
    }
}

fn short_type(t: &str) -> String {
    t.rsplit("::").next().unwrap_or(t).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sui_clmm_manager::Venue;

    fn sample_pool(id: &str, a: &str, b: &str, liq: u128) -> PoolRow {
        PoolRow {
            venue: Venue::Bluefin,
            object_id: id.into(),
            name: format!("{a}-{b}"),
            coin_type_a: a.into(),
            coin_type_b: b.into(),
            symbol_a: "A".into(),
            symbol_b: "B".into(),
            decimals_a: 9,
            decimals_b: 6,
            fee_rate: 2500,
            tick_spacing: 60,
            liquidity: liq,
            coin_a: 0,
            coin_b: 0,
            current_tick: 0,
            sqrt_price: 0,
            is_paused: false,
            rank: 1,
        }
    }

    #[test]
    fn picks_highest_liquidity_direct() {
        let universe = RouterUniverse {
            pools: vec![
                sample_pool("0x1", "0x2::sui::SUI", "0x3::usdc::USDC", 100),
                sample_pool("0x2", "0x2::sui::SUI", "0x3::usdc::USDC", 9_000),
            ],
        };
        let router = Router::new(universe);
        let route = router
            .plan_swap("0x2::sui::SUI", "0x3::usdc::USDC", 1_000)
            .unwrap();
        assert_eq!(route.hops.len(), 1);
        assert_eq!(route.hops[0].pool_id, "0x2");
        assert!(route.hops[0].a_to_b);
    }
}
