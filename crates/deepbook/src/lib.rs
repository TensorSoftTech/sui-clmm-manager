//! DeepBook — stub (router CLOB leg later). No LP positions.

#![forbid(unsafe_code)]

use anyhow::Result;
use sui_clmm_manager::{
    GraphQl, PoolDetail, PoolRow, PositionDetail, PositionRow, Store, Venue,
};

pub fn venue() -> Venue {
    Venue::Deepbook
}

pub fn status() -> &'static str {
    "deepbook · stub · router CLOB leg later"
}

pub fn list_positions(
    _gql: &GraphQl<'_>,
    _store: Option<&Store>,
    _owner: &str,
) -> Result<Vec<PositionRow>> {
    Ok(vec![])
}

pub fn fetch_position_detail(
    _gql: &GraphQl<'_>,
    _store: Option<&Store>,
    _row: &PositionRow,
) -> Result<PositionDetail> {
    anyhow::bail!("deepbook has no CLMM positions")
}

pub fn list_top_pools(
    _gql: &GraphQl<'_>,
    _store: Option<&Store>,
    _limit: usize,
) -> Result<Vec<PoolRow>> {
    Ok(vec![])
}

pub fn fetch_pool_detail(
    _gql: &GraphQl<'_>,
    _store: Option<&Store>,
    _object_id: &str,
) -> Result<PoolDetail> {
    anyhow::bail!("deepbook pool detail not implemented")
}
