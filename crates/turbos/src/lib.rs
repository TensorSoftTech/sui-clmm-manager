//! Turbos CLMM — wallet position NFTs + inner position (ticks/liquidity).

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use serde_json::Value;
use sui_clmm_manager::{
    CachedToken, GraphQl, PoolDetail, PoolRow, PositionDetail, PositionRow, Store, Venue,
    contents_json, crude_symbol, detail_from_row, i32_bits, json_bool, json_str, json_u64,
    json_u128, normalize_coin_type,
};

pub const PACKAGE_ID: &str =
    "0x91bfbc386a41afcfd9b2533058d7e915a1d3829089cc268ff4333d54d6339ca1";

/// Owned NFT that points at the real position object.
pub const POSITION_NFT_TYPE: &str =
    "0x91bfbc386a41afcfd9b2533058d7e915a1d3829089cc268ff4333d54d6339ca1::position_nft::TurbosPositionNFT";

pub fn venue() -> Venue {
    Venue::Turbos
}

pub fn status() -> &'static str {
    "turbos · position NFTs"
}

pub fn list_positions(
    gql: &GraphQl<'_>,
    store: Option<&Store>,
    owner: &str,
) -> Result<Vec<PositionRow>> {
    let nodes = gql
        .owned_objects(owner, POSITION_NFT_TYPE, 50, 20)
        .context("turbos owned NFTs")?;
    let mut rows = Vec::new();
    for node in &nodes {
        let Some(mut row) = decode_nft(node) else {
            continue;
        };
        // Fill ticks / liquidity from inner position object.
        if !row.inner_position_id.is_empty()
            && let Ok(inner) = gql.object_json(&row.inner_position_id)
            && let Some(json) = contents_json(&inner)
        {
            row.tick_lower = i32_bits(json.get("tick_lower_index"));
            row.tick_upper = i32_bits(json.get("tick_upper_index"));
            row.liquidity = json_u128(json.get("liquidity"));
            row.fee_owed_a = json_u128(json.get("tokens_owed_a"));
            row.fee_owed_b = json_u128(json.get("tokens_owed_b"));
        }
        enrich_symbols(gql, store, &mut row);
        rows.push(row);
    }
    Ok(rows)
}

pub fn fetch_position_detail(
    gql: &GraphQl<'_>,
    store: Option<&Store>,
    row: &PositionRow,
) -> Result<PositionDetail> {
    let mut row = row.clone();
    if !row.inner_position_id.is_empty()
        && let Ok(inner) = gql.object_json(&row.inner_position_id)
        && let Some(json) = contents_json(&inner)
    {
        row.tick_lower = i32_bits(json.get("tick_lower_index"));
        row.tick_upper = i32_bits(json.get("tick_upper_index"));
        row.liquidity = json_u128(json.get("liquidity"));
        row.fee_owed_a = json_u128(json.get("tokens_owed_a"));
        row.fee_owed_b = json_u128(json.get("tokens_owed_b"));
    }
    enrich_symbols(gql, store, &mut row);
    let pool = gql.object_json(&row.pool_id).context("turbos pool")?;
    let json = contents_json(&pool).cloned().unwrap_or(Value::Null);
    let current_tick = i32_bits(
        json.get("tick_current_index")
            .or_else(|| json.get("current_tick_index"))
            .or_else(|| json.get("tick_index")),
    );
    let tick_spacing = json_u64(json.get("tick_spacing")) as u32;
    let mut detail = detail_from_row(
        row,
        current_tick,
        tick_spacing,
        json_u128(json.get("liquidity")),
        json_u128(
            json.get("sqrt_price")
                .or_else(|| json.get("current_sqrt_price")),
        ),
    );
    detail.coin_a_reserve = json_u128(json.get("coin_a"));
    detail.coin_b_reserve = json_u128(json.get("coin_b"));
    detail.is_paused = json_bool(json.get("is_pause"));
    detail.rebuild_series();
    detail.note = format!(
        "turbos · inner {} · chart {} · fees = stored owed",
        sui_clmm_manager::short_id(&detail.row.inner_position_id),
        detail.series.len()
    );
    Ok(detail)
}

fn decode_nft(node: &Value) -> Option<PositionRow> {
    let object_id = node.get("address")?.as_str()?.to_string();
    let json = contents_json(node)?;
    let coin_a = normalize_coin_type(&json_str(json.get("coin_type_a")));
    let coin_b = normalize_coin_type(&json_str(json.get("coin_type_b")));
    Some(PositionRow {
        venue: Venue::Turbos,
        object_id,
        pool_id: json_str(json.get("pool_id")),
        pool_label: format!("{}/{}", crude_symbol(&coin_a), crude_symbol(&coin_b)),
        coin_type_a: coin_a.clone(),
        coin_type_b: coin_b.clone(),
        symbol_a: crude_symbol(&coin_a),
        symbol_b: crude_symbol(&coin_b),
        decimals_a: 0,
        decimals_b: 0,
        tick_lower: 0,
        tick_upper: 0,
        liquidity: 0,
        fee_rate: 0,
        fee_owed_a: 0,
        fee_owed_b: 0,
        current_tick: None,
        tick_spacing: None,
        in_range: None,
        inner_position_id: json_str(json.get("position_id")),
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
    row.pool_label = format!("{}/{}", row.symbol_a, row.symbol_b);
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
    anyhow::bail!("turbos pool list deferred — use later with router prefetch")
}
