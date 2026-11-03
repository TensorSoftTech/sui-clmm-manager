//! Cetus CLMM — wallet positions via GraphQL (+ pool stub for later).

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use serde_json::Value;
use sui_clmm_manager::{
    CachedToken, GraphQl, PoolDetail, PoolRow, PositionDetail, PositionRow, Store, Venue,
    contents_json, crude_symbol, detail_from_row, i32_bits, json_bool, json_str, json_u64,
    json_u128, normalize_coin_type,
};

pub const PACKAGE_ID: &str =
    "0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb";

pub const POSITION_TYPE: &str =
    "0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb::position::Position";

pub fn venue() -> Venue {
    Venue::Cetus
}

pub fn status() -> &'static str {
    "cetus · positions"
}

pub fn list_positions(
    gql: &GraphQl<'_>,
    store: Option<&Store>,
    owner: &str,
) -> Result<Vec<PositionRow>> {
    let nodes = gql
        .owned_objects(owner, POSITION_TYPE, 50, 20)
        .context("cetus owned positions")?;
    let mut rows = Vec::new();
    for node in &nodes {
        if let Some(mut row) = decode_position(node) {
            enrich_symbols(gql, store, &mut row);
            rows.push(row);
        }
    }
    Ok(rows)
}

pub fn fetch_position_detail(
    gql: &GraphQl<'_>,
    store: Option<&Store>,
    row: &PositionRow,
) -> Result<PositionDetail> {
    let mut row = row.clone();
    enrich_symbols(gql, store, &mut row);
    let pool = gql.object_json(&row.pool_id).context("cetus pool")?;
    let json = contents_json(&pool).cloned().unwrap_or(Value::Null);
    let current_tick = i32_bits(
        json.get("current_tick_index")
            .or_else(|| json.get("tick_index")),
    );
    let tick_spacing = json_u64(json.get("tick_spacing")) as u32;
    let mut detail = detail_from_row(
        row,
        current_tick,
        tick_spacing,
        json_u128(json.get("liquidity")),
        json_u128(
            json.get("current_sqrt_price")
                .or_else(|| json.get("sqrt_price")),
        ),
    );
    detail.coin_a_reserve = json_u128(json.get("coin_a"));
    detail.coin_b_reserve = json_u128(json.get("coin_b"));
    detail.is_paused = json_bool(json.get("is_pause").or_else(|| json.get("is_paused")));
    detail.rebuild_series();
    detail.note = format!(
        "cetus · chart {} · fees usually off-NFT (stored {})",
        detail.series.len(),
        if detail.row.fee_owed_a == 0 && detail.row.fee_owed_b == 0 {
            "0"
        } else {
            "owed"
        }
    );
    Ok(detail)
}

fn decode_position(node: &Value) -> Option<PositionRow> {
    let object_id = node.get("address")?.as_str()?.to_string();
    let json = contents_json(node)?;
    let coin_a = normalize_coin_type(&json_str(json.get("coin_type_a")));
    let coin_b = normalize_coin_type(&json_str(json.get("coin_type_b")));
    Some(PositionRow {
        venue: Venue::Cetus,
        object_id,
        pool_id: json_str(json.get("pool")),
        pool_label: json_str(json.get("name")),
        coin_type_a: coin_a.clone(),
        coin_type_b: coin_b.clone(),
        symbol_a: crude_symbol(&coin_a),
        symbol_b: crude_symbol(&coin_b),
        decimals_a: 0,
        decimals_b: 0,
        tick_lower: i32_bits(json.get("tick_lower_index")),
        tick_upper: i32_bits(json.get("tick_upper_index")),
        liquidity: json_u128(json.get("liquidity")),
        fee_rate: 0,
        fee_owed_a: 0,
        fee_owed_b: 0,
        current_tick: None,
        tick_spacing: None,
        in_range: None,
        inner_position_id: String::new(),
    })
}

fn enrich_symbols(gql: &GraphQl<'_>, store: Option<&Store>, row: &mut PositionRow) {
    if let Some((s, d)) = resolve_coin(gql, store, &row.coin_type_a) {
        row.symbol_a = s;
        row.decimals_a = d;
    }
    if let Some((s, d)) = resolve_coin(gql, store, &row.coin_type_b) {
        row.symbol_b = s;
        row.decimals_b = d;
    }
}

fn resolve_coin(
    gql: &GraphQl<'_>,
    store: Option<&Store>,
    coin_type: &str,
) -> Option<(String, u8)> {
    if coin_type.is_empty() {
        return None;
    }
    if let Some(store) = store
        && let Ok(Some(c)) = store.get_token(coin_type)
    {
        return Some((c.symbol, c.decimals));
    }
    let q = r#"query($type: String!) { coinMetadata(coinType: $type) { symbol decimals } }"#;
    let data = gql
        .post(
            "coinMetadata",
            &crude_symbol(coin_type),
            q,
            serde_json::json!({ "type": coin_type }),
        )
        .ok()?;
    let meta = data.get("coinMetadata")?;
    let symbol = meta
        .get("symbol")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| crude_symbol(coin_type));
    let decimals = meta.get("decimals").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
    if let Some(store) = store {
        let _ = store.upsert_token(&CachedToken {
            coin_type: coin_type.into(),
            symbol: symbol.clone(),
            decimals,
        });
    }
    Some((symbol, decimals))
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
    anyhow::bail!("cetus pool list deferred — use later with router prefetch")
}
