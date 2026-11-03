//! Bluefin Spot CLMM — GraphQL pool prefetch + wallet positions.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use sui_clmm_manager::{
    CachedToken, GraphQl, PoolDetail, PoolRow, PositionDetail, PositionRow, RewardLine, Store,
    Venue, contents_json, crude_symbol, detail_from_row, format_amount, i32_bits, i32_from_bits,
    json_bool, json_str, json_u64, json_u128, normalize_coin_type, parse_pool_type_params,
};

pub const PACKAGE_ID: &str =
    "0x3492c874c1e3b3e2984e8c41b589e642d4d0a5d6459e5a9cfc2d52fd7c89c267";

pub const POOL_TYPE_FILTER: &str =
    "0x3492c874c1e3b3e2984e8c41b589e642d4d0a5d6459e5a9cfc2d52fd7c89c267::pool::Pool";

pub const POSITION_TYPE: &str =
    "0x3492c874c1e3b3e2984e8c41b589e642d4d0a5d6459e5a9cfc2d52fd7c89c267::position::Position";

const PAGE_SIZE: u32 = 50;

pub fn venue() -> Venue {
    Venue::Bluefin
}

pub fn status() -> &'static str {
    "bluefin · positions + GraphQL pools"
}

const COIN_META_QUERY: &str = r#"
query CoinMeta($type: String!) {
  coinMetadata(coinType: $type) {
    symbol
    decimals
  }
}
"#;

const OBJECTS_QUERY: &str = r#"
query BluefinPools($type: String!, $first: Int!, $after: String) {
  objects(first: $first, after: $after, filter: { type: $type }) {
    pageInfo { hasNextPage endCursor }
    nodes {
      address
      asMoveObject {
        contents {
          type { repr }
          json
        }
      }
    }
  }
}
"#;

/// Wallet-owned Bluefin positions.
pub fn list_positions(
    gql: &GraphQl<'_>,
    store: Option<&Store>,
    owner: &str,
) -> Result<Vec<PositionRow>> {
    let nodes = gql
        .owned_objects(owner, POSITION_TYPE, 50, 20)
        .context("bluefin owned positions")?;
    let mut rows = Vec::with_capacity(nodes.len());
    for node in &nodes {
        if let Some(mut row) = decode_position_node(node) {
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
    let pool = gql.object_json(&row.pool_id).context("bluefin pool")?;
    let json = contents_json(&pool).cloned().unwrap_or(Value::Null);
    let current_tick = i32_bits(json.get("current_tick_index"));
    let tick_spacing = json
        .pointer("/ticks_manager/tick_spacing")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let mut detail = detail_from_row(
        row,
        current_tick,
        tick_spacing,
        json_u128(json.get("liquidity")),
        json_u128(json.get("current_sqrt_price")),
    );
    detail.coin_a_reserve = json_u128(json.get("coin_a"));
    detail.coin_b_reserve = json_u128(json.get("coin_b"));
    detail.is_paused = json_bool(json.get("is_paused"));
    detail.ticks = fetch_nearby_ticks(gql, &json, detail.row.tick_lower, detail.row.tick_upper)?;
    detail.rebuild_series();

    let growth_a = json_u128(json.get("fee_growth_global_coin_a"));
    let growth_b = json_u128(json.get("fee_growth_global_coin_b"));
    // Re-read position for checkpoints (row may only have owed).
    let pos_node = gql.object_json(&detail.row.object_id).ok();
    let pos_json = pos_node
        .as_ref()
        .and_then(|n| contents_json(n))
        .cloned()
        .unwrap_or(Value::Null);
    let checkpoint_a = json_u128(pos_json.get("fee_growth_coin_a"));
    let checkpoint_b = json_u128(pos_json.get("fee_growth_coin_b"));
    let (lo_a, lo_b) = tick_outside(&detail.ticks, detail.row.tick_lower);
    let (hi_a, hi_b) = tick_outside(&detail.ticks, detail.row.tick_upper);
    if lo_a.is_some() && hi_a.is_some() {
        detail.apply_fee_quote(
            growth_a,
            growth_b,
            checkpoint_a,
            checkpoint_b,
            lo_a.unwrap_or(0),
            lo_b.unwrap_or(0),
            hi_a.unwrap_or(0),
            hi_b.unwrap_or(0),
        );
    }

    detail.rewards = collect_bluefin_rewards(
        &json,
        &pos_json,
        &detail.ticks,
        detail.row.tick_lower,
        detail.row.tick_upper,
        detail.current_tick,
        detail.row.liquidity,
        detail.pool_liquidity,
    );

    let reward_n = detail.rewards.len();
    let reward_claim = detail.rewards.iter().filter(|r| r.claim_ok).count();
    detail.note = if detail.ticks.is_empty() {
        format!("synthetic chart · fees/rewards stored · rewards {reward_n}")
    } else if detail.fees_claim_ok {
        format!(
            "{} ticks · fees claim-sim · rewards {reward_claim}/{reward_n} · chart {}",
            detail.ticks.len(),
            detail.series.len()
        )
    } else {
        format!(
            "{} ticks · fees stored · rewards {reward_n} · chart {}",
            detail.ticks.len(),
            detail.series.len()
        )
    };
    Ok(detail)
}

fn collect_bluefin_rewards(
    pool_json: &Value,
    pos_json: &Value,
    ticks: &[sui_clmm_manager::TickSample],
    tick_lower: i32,
    tick_upper: i32,
    tick_current: i32,
    liquidity: u128,
    pool_liquidity: u128,
) -> Vec<RewardLine> {
    let pool_rewards = pool_json
        .get("reward_infos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let pos_rewards = pos_json
        .get("reward_infos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if pool_rewards.is_empty() {
        return Vec::new();
    }

    let lo = ticks.iter().find(|t| t.index == tick_lower);
    let hi = ticks.iter().find(|t| t.index == tick_upper);
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut out = Vec::new();
    for (i, pool_r) in pool_rewards.iter().enumerate() {
        let coin_type = normalize_coin_type(&json_str(
            pool_r
                .get("reward_coin_type")
                .or_else(|| pool_r.get("coin_type")),
        ));
        if coin_type.is_empty() || coin_type == "0x" {
            continue;
        }
        let symbol = {
            let s = json_str(pool_r.get("reward_coin_symbol"));
            if s.is_empty() {
                crude_symbol(&coin_type)
            } else {
                s
            }
        };
        let decimals = json_u64(pool_r.get("reward_coin_decimals")).min(255) as u8;
        // Accrue like on-chain `update_reward_infos` before quoting the position.
        let growth_global = sui_clmm_manager::accrue_reward_growth_global(
            json_u128(pool_r.get("reward_growth_global")),
            json_u128(pool_r.get("reward_per_seconds")),
            json_u64(pool_r.get("last_update_time")),
            json_u64(pool_r.get("ended_at_seconds")),
            now_secs,
            pool_liquidity,
        );
        let pos_r = pos_rewards.get(i);
        let owed = pos_r
            .map(|r| json_u128(r.get("coins_owed_reward")))
            .unwrap_or(0);
        let checkpoint = pos_r
            .map(|r| json_u128(r.get("reward_growth_inside_last")))
            .unwrap_or(0);

        let (amount, claim_ok) = if let (Some(lo), Some(hi)) = (lo, hi) {
            let out_lo = lo.reward_growth_outside.get(i).copied().unwrap_or(0);
            let out_hi = hi.reward_growth_outside.get(i).copied().unwrap_or(0);
            let qty = sui_clmm_manager::collect_fees_quote(
                owed,
                growth_global,
                checkpoint,
                out_lo,
                out_hi,
                tick_lower,
                tick_upper,
                tick_current,
                liquidity,
            );
            (qty, true)
        } else {
            (owed, false)
        };

        if amount == 0 {
            continue;
        }

        out.push(RewardLine {
            coin_type,
            symbol: symbol.clone(),
            decimals,
            amount,
            amount_ui: format!("{} {}", format_amount(amount, decimals), symbol),
            usd: None,
            claim_ok,
        });
    }
    out
}

fn tick_outside(ticks: &[sui_clmm_manager::TickSample], index: i32) -> (Option<u128>, Option<u128>) {
    ticks
        .iter()
        .find(|t| t.index == index)
        .map(|t| (Some(t.fee_growth_outside_a), Some(t.fee_growth_outside_b)))
        .unwrap_or((None, None))
}

fn decode_position_node(node: &Value) -> Option<PositionRow> {
    let object_id = node.get("address")?.as_str()?.to_string();
    let json = contents_json(node)?;
    let coin_a = normalize_coin_type(&json_str(json.get("coin_type_a")));
    let coin_b = normalize_coin_type(&json_str(json.get("coin_type_b")));
    let name = json_str(json.get("name"));
    Some(PositionRow {
        venue: Venue::Bluefin,
        object_id,
        pool_id: json_str(json.get("pool_id")),
        pool_label: name,
        coin_type_a: coin_a.clone(),
        coin_type_b: coin_b.clone(),
        symbol_a: crude_symbol(&coin_a),
        symbol_b: crude_symbol(&coin_b),
        decimals_a: 0,
        decimals_b: 0,
        tick_lower: i32_bits(json.get("lower_tick")),
        tick_upper: i32_bits(json.get("upper_tick")),
        liquidity: json_u128(json.get("liquidity")),
        fee_rate: json_u64(json.get("fee_rate")) as u32,
        fee_owed_a: json_u128(json.get("token_a_fee")),
        fee_owed_b: json_u128(json.get("token_b_fee")),
        current_tick: None,
        tick_spacing: None,
        in_range: None,
        inner_position_id: String::new(),
    })
}

/// Read Bluefin `Table<I32, TickInfo>` via `address.dynamicFields` (object DF is null).
fn fetch_nearby_ticks(
    gql: &GraphQl<'_>,
    pool_json: &Value,
    tick_lower: i32,
    tick_upper: i32,
) -> Result<Vec<sui_clmm_manager::TickSample>> {
    let Some(table_id) = pool_json
        .pointer("/ticks_manager/ticks/id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            pool_json
                .pointer("/ticks_manager/ticks/id/id")
                .and_then(|v| v.as_str())
        })
    else {
        return Ok(vec![]);
    };
    // Table UIDs are not full objects — must query Address.dynamicFields.
    let q = r#"
    query Ticks($id: SuiAddress!, $after: String) {
      address(address: $id) {
        dynamicFields(first: 50, after: $after) {
          pageInfo { hasNextPage endCursor }
          nodes {
            name { json }
            value {
              ... on MoveValue { json }
              ... on MoveObject { contents { json } }
            }
          }
        }
      }
    }
    "#;
    let mut samples = Vec::new();
    let mut after: Option<String> = None;
    for _page in 0..8 {
        let vars = json!({ "id": table_id, "after": after });
        let data = match gql.post("dynamicFields", "bluefin.ticks", q, vars) {
            Ok(d) => d,
            Err(_) => break,
        };
        let nodes = data
            .pointer("/address/dynamicFields/nodes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if nodes.is_empty() {
            break;
        }
        for node in nodes {
            let idx = parse_tick_name(node.get("name")).or_else(|| {
                tick_value_json(&node)
                    .and_then(|j| j.get("index").or_else(|| j.get("tick_index")))
                    .map(|v| i32_bits(Some(v)))
            });
            let Some(index) = idx else { continue };
            let Some(j) = tick_value_json(&node) else { continue };
            let gross = json_u128(j.get("liquidity_gross"));
            let net = parse_i128(j.get("liquidity_net"));
            let outside_a = json_u128(
                j.get("fee_growth_outside_a")
                    .or_else(|| j.get("fee_growth_outside_coin_a")),
            );
            let outside_b = json_u128(
                j.get("fee_growth_outside_b")
                    .or_else(|| j.get("fee_growth_outside_coin_b")),
            );
            let reward_growth_outside = j
                .get("reward_growths_outside")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().map(|x| json_u128(Some(x))).collect())
                .unwrap_or_default();
            samples.push(sui_clmm_manager::TickSample {
                index,
                liquidity_gross: gross,
                liquidity_net: net,
                fee_growth_outside_a: outside_a,
                fee_growth_outside_b: outside_b,
                reward_growth_outside,
            });
        }
        let has_next = data
            .pointer("/address/dynamicFields/pageInfo/hasNextPage")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !has_next {
            break;
        }
        after = data
            .pointer("/address/dynamicFields/pageInfo/endCursor")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if after.is_none() {
            break;
        }
    }
    samples.sort_by_key(|t| t.index);
    samples.dedup_by_key(|t| t.index);
    // Keep pool neighborhood around spot + position (not only ±2000 of bounds).
    let pad = 5000;
    let lo = tick_lower.min(tick_upper) - pad;
    let hi = tick_lower.max(tick_upper) + pad;
    samples.retain(|t| t.index >= lo && t.index <= hi);
    Ok(samples)
}

fn tick_value_json(node: &Value) -> Option<&Value> {
    node.pointer("/value/json")
        .or_else(|| node.pointer("/value/contents/json"))
}

fn parse_tick_name(name: Option<&Value>) -> Option<i32> {
    let name = name?;
    let n = name.get("json").unwrap_or(name);
    match n {
        Value::Number(num) => Some(num.as_i64().unwrap_or(0) as i32),
        Value::String(s) => s.parse().ok(),
        Value::Object(_) => Some(i32_bits(Some(n))),
        _ => None,
    }
}

fn parse_i128(v: Option<&Value>) -> i128 {
    let Some(v) = v else { return 0 };
    match v {
        Value::String(s) => s.parse().unwrap_or(0),
        Value::Number(num) => num.as_i64().unwrap_or(0) as i128,
        Value::Object(o) => {
            // IntegerMate I128 { bits: u128 as string/number }
            let bits = o
                .get("bits")
                .map(|b| match b {
                    Value::String(s) => s.parse::<u128>().unwrap_or(0),
                    Value::Number(n) => n.as_u64().unwrap_or(0) as u128,
                    _ => 0,
                })
                .unwrap_or(0);
            bits as i128
        }
        _ => 0,
    }
}

// ── pool prefetch (kept for later / router; not used by TUI v0) ─────────────

pub fn list_top_pools(
    gql: &GraphQl<'_>,
    store: Option<&Store>,
    limit: usize,
) -> Result<Vec<PoolRow>> {
    let limit = limit.clamp(1, 500);
    let mut all = Vec::new();
    let mut after: Option<String> = None;
    let mut pages = 0u32;

    loop {
        pages += 1;
        if pages > 40 {
            break;
        }
        let vars = json!({
            "type": POOL_TYPE_FILTER,
            "first": PAGE_SIZE,
            "after": after,
        });
        let data = gql
            .post("objects", "bluefin.Pool", OBJECTS_QUERY, vars)
            .context("bluefin objects page")?;
        let objects = data
            .get("objects")
            .ok_or_else(|| anyhow!("missing objects"))?;
        let nodes = objects
            .get("nodes")
            .and_then(|n| n.as_array())
            .ok_or_else(|| anyhow!("missing nodes"))?;

        for node in nodes {
            if let Some(row) = decode_pool_node(node) {
                all.push(row);
            }
        }

        let page_info = objects.get("pageInfo");
        let has_next = page_info
            .and_then(|p| p.get("hasNextPage"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !has_next {
            break;
        }
        after = page_info
            .and_then(|p| p.get("endCursor"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if after.is_none() {
            break;
        }
    }

    all.sort_by(|a, b| b.liquidity.cmp(&a.liquidity));
    all.truncate(limit);

    for (i, row) in all.iter_mut().enumerate() {
        row.rank = (i + 1) as u32;
        enrich_pool_symbols(gql, store, row);
    }

    Ok(all)
}

fn enrich_pool_symbols(gql: &GraphQl<'_>, store: Option<&Store>, row: &mut PoolRow) {
    if let Some((sym, dec)) = resolve_coin(gql, store, &row.coin_type_a) {
        row.symbol_a = sym;
        row.decimals_a = dec;
    }
    if let Some((sym, dec)) = resolve_coin(gql, store, &row.coin_type_b) {
        row.symbol_b = sym;
        row.decimals_b = dec;
    }
}

fn enrich_symbols(gql: &GraphQl<'_>, store: Option<&Store>, row: &mut PositionRow) {
    if let Some((sym, dec)) = resolve_coin(gql, store, &row.coin_type_a) {
        row.symbol_a = sym;
        row.decimals_a = dec;
    }
    if let Some((sym, dec)) = resolve_coin(gql, store, &row.coin_type_b) {
        row.symbol_b = sym;
        row.decimals_b = dec;
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
        && let Ok(Some(cached)) = store.get_token(coin_type)
    {
        return Some((cached.symbol, cached.decimals));
    }

    let vars = json!({ "type": coin_type });
    let data = gql
        .post(
            "coinMetadata",
            &crude_symbol(coin_type),
            COIN_META_QUERY,
            vars,
        )
        .ok()?;
    let meta = data.get("coinMetadata")?;
    let symbol = meta
        .get("symbol")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let decimals = meta
        .get("decimals")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u8;
    let symbol = if symbol.is_empty() {
        crude_symbol(coin_type)
    } else {
        symbol
    };

    if let Some(store) = store {
        let _ = store.upsert_token(&CachedToken {
            coin_type: coin_type.to_string(),
            symbol: symbol.clone(),
            decimals,
        });
    }
    Some((symbol, decimals))
}

fn decode_pool_node(node: &Value) -> Option<PoolRow> {
    let object_id = node.get("address")?.as_str()?.to_string();
    let contents = node
        .pointer("/asMoveObject/contents")
        .or_else(|| node.get("asMoveObject")?.get("contents"))?;
    let type_repr = contents
        .pointer("/type/repr")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let json = contents.get("json")?;

    let (coin_type_a, coin_type_b) = parse_pool_type_params(type_repr).unwrap_or_else(|| {
        ("unknown::A".into(), "unknown::B".into())
    });

    let name = json_str(json.get("name"));
    let fee_rate = json_u64(json.get("fee_rate")) as u32;
    let tick_spacing = json
        .pointer("/ticks_manager/tick_spacing")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let liquidity = json_u128(json.get("liquidity"));
    let coin_a = json_u128(json.get("coin_a"));
    let coin_b = json_u128(json.get("coin_b"));
    let sqrt_price = json_u128(json.get("current_sqrt_price"));
    let tick_bits = json
        .pointer("/current_tick_index/bits")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let current_tick = i32_from_bits(tick_bits);
    let is_paused = json_bool(json.get("is_paused"));

    Some(PoolRow {
        venue: Venue::Bluefin,
        object_id,
        name,
        coin_type_a: coin_type_a.clone(),
        coin_type_b: coin_type_b.clone(),
        symbol_a: crude_symbol(&coin_type_a),
        symbol_b: crude_symbol(&coin_type_b),
        decimals_a: 0,
        decimals_b: 0,
        fee_rate,
        tick_spacing,
        liquidity,
        coin_a,
        coin_b,
        current_tick,
        sqrt_price,
        is_paused,
        rank: 0,
    })
}

pub fn fetch_pool_detail(
    gql: &GraphQl<'_>,
    store: Option<&Store>,
    object_id: &str,
) -> Result<PoolDetail> {
    let node = gql.object_json(object_id)?;
    let mut row = decode_pool_node(&node).ok_or_else(|| anyhow!("decode pool failed"))?;
    enrich_pool_symbols(gql, store, &mut row);
    let json = contents_json(&node).cloned().unwrap_or(Value::Null);
    Ok(PoolDetail {
        fee_growth_global_a: json_u128(json.get("fee_growth_global_coin_a")),
        fee_growth_global_b: json_u128(json.get("fee_growth_global_coin_b")),
        protocol_fee_a: json_u128(json.get("protocol_fee_coin_a")),
        protocol_fee_b: json_u128(json.get("protocol_fee_coin_b")),
        protocol_fee_share: json_u64(json.get("protocol_fee_share")) as u32,
        position_index: json_u64(json.get("position_index")),
        sequence_number: json_u64(json.get("sequence_number")),
        icon_url: json_str(json.get("icon_url")),
        row,
    })
}
