//! Shared foundation for Sui concentrated-liquidity tools.
//!
//! - [`graphql`] — metered GraphQL client (+ gRPC stub for later sim/send)
//! - [`store`] — SQLite settings + token cache + pool snapshots
//! - [`pool`] — `PoolRow` / `PoolDetail` / prefetch stats
//! - [`venue`] — venue tags + filter

#![forbid(unsafe_code)]

pub mod chart;
pub mod clmm_math;
pub mod decode;
pub mod fees;
pub mod graphql;
pub mod pool;
pub mod position;
pub mod pricing;
pub mod quotes;
pub mod store;
pub mod venue;

pub use chart::{
    CHART_FETCH_HALF_SPACINGS, CHART_HALF_SPACINGS_DEFAULT, CHART_HALF_SPACINGS_MIN,
    LiquiditySample, TickPoint, TokenSide, build_liquidity_series, chart_view_window, scale_u128,
    series_from_ticks, slice_liquidity_series,
};
pub use clmm_math::{amounts_from_liquidity, get_sqrt_price_at_tick, mul_shift_right_64};
pub use fees::{
    accrue_reward_growth_global, collect_fees_quote, fee_growth_inside, tokens_owed_delta,
};
pub use pricing::{
    PriceMode, TokenUsd, UsdEstimate, estimate_position_usd, fetch_coin_usd_prices, format_usd,
    format_usd_axis, liquidity_l_to_usd, raw_to_usd,
};
pub use quotes::aftermath::{
    AFTERMATH_PRICES_URL, fetch_usd_prices as aftermath_usd_prices,
    fetch_usd_prices_info as aftermath_price_info,
};
pub use decode::{
    contents_json, contents_type, i32_bits, json_bool, json_str, json_u64, json_u128,
    normalize_coin_type,
};
pub use graphql::{
    DEFAULT_GRAPHQL_URL, GRAPHQL_HISTORY_CAP, GraphQl, GraphQlCall, GraphQlMeter, grpc_stub,
    is_public_graphql,
};
pub use pool::{
    PoolDetail, PoolRow, PrefetchStats, crude_symbol, format_amount, format_compact_u128,
    i32_from_bits, parse_pool_type_params, short_id,
};
pub use position::{
    PositionDetail, PositionRow, PositionSnapshotRow, RewardLine, TickSample, detail_from_row,
};
pub use store::{
    APP_DIR_NAME, AppSettings, CachedToken, DEFAULT_DB_FILE, Store, default_config_dir,
    default_db_path, display_path, expand_tilde, resolve_existing_db, write_active_pointer,
};
pub use venue::{Venue, VenueFilter};

/// Suiscan object URL (mainnet).
pub fn suiscan_object_url(object_id: &str) -> String {
    format!("https://suiscan.xyz/mainnet/object/{object_id}")
}
