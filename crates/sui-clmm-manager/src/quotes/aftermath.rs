//! Aftermath Finance REST prices (Sui USD), analogous to Sol Jupiter Price API.
//!
//! Docs: https://docs.aftermath.finance/for-developers/api/rest-api/prices
//!
//! ```text
//! POST https://aftermath.finance/api/prices
//! { "coinTypes": ["0x2::sui::SUI", …] } → Record<coinType, number>
//! ```

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const AFTERMATH_PRICES_URL: &str = "https://aftermath.finance/api/prices";
pub const AFTERMATH_PRICE_INFO_URL: &str = "https://aftermath.finance/api/price-info";

const MIN_INTERVAL: Duration = Duration::from_millis(200);
static LAST_CALL: Mutex<Option<Instant>> = Mutex::new(None);

fn throttle() {
    let mut last = LAST_CALL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(prev) = *last {
        let elapsed = prev.elapsed();
        if elapsed < MIN_INTERVAL {
            std::thread::sleep(MIN_INTERVAL - elapsed);
        }
    }
    *last = Some(Instant::now());
}

fn http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("sui-clmm-manager/0.1 (aftermath prices)")
        .build()
        .context("aftermath http client")
}

/// Canonicalize a Move coin type for Aftermath (`0x` prefix; keep address hex as-is).
pub fn canonicalize_coin_type(coin_type: &str) -> String {
    crate::decode::normalize_coin_type(coin_type.trim())
}

fn lookup_price(map: &HashMap<String, f64>, coin_type: &str) -> Option<f64> {
    let keys = [
        coin_type.to_string(),
        canonicalize_coin_type(coin_type),
        crate::decode::normalize_coin_type(coin_type),
    ];
    for k in &keys {
        if let Some(&p) = map.get(k)
            && p.is_finite()
            && p > 0.0
        {
            return Some(p);
        }
        // Case-insensitive scan (Aftermath may change casing on package ids).
        let kl = k.to_ascii_lowercase();
        for (rk, &p) in map {
            if rk.to_ascii_lowercase() == kl && p.is_finite() && p > 0.0 {
                return Some(p);
            }
        }
    }
    None
}

/// Batch USD prices. Returns map keyed by **request** coin types (best-effort match).
pub fn fetch_usd_prices(coin_types: &[String]) -> Result<HashMap<String, f64>> {
    if coin_types.is_empty() {
        return Ok(HashMap::new());
    }
    throttle();

    let mut seen = std::collections::HashSet::new();
    let mut req_types = Vec::new();
    for t in coin_types {
        let c = canonicalize_coin_type(t);
        if seen.insert(c.clone()) {
            req_types.push(c);
        }
    }

    let resp = http_client()?
        .post(AFTERMATH_PRICES_URL)
        .header("Accept", "application/json")
        .json(&json!({ "coinTypes": req_types }))
        .send()
        .context("aftermath /api/prices HTTP")?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        bail!(
            "aftermath /api/prices HTTP {status}: {}",
            truncate(&text, 180)
        );
    }
    let value: Value = serde_json::from_str(&text).context("aftermath /api/prices JSON")?;
    let mut raw = HashMap::new();
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            if let Some(p) = v.as_f64()
                && p.is_finite()
                && p > 0.0
            {
                raw.insert(k.clone(), p);
            }
        }
    }

    let mut out = HashMap::new();
    for t in coin_types {
        if let Some(p) = lookup_price(&raw, t) {
            out.insert(t.clone(), p);
        }
    }
    Ok(out)
}

/// Same as [`fetch_usd_prices`] plus optional 24h change via `/api/price-info`.
pub fn fetch_usd_prices_info(coin_types: &[String]) -> Result<HashMap<String, (f64, f64)>> {
    if coin_types.is_empty() {
        return Ok(HashMap::new());
    }
    throttle();

    let mut seen = std::collections::HashSet::new();
    let mut req_types = Vec::new();
    for t in coin_types {
        let c = canonicalize_coin_type(t);
        if seen.insert(c.clone()) {
            req_types.push(c);
        }
    }

    let resp = http_client()?
        .post(AFTERMATH_PRICE_INFO_URL)
        .header("Accept", "application/json")
        .json(&json!({ "coins": req_types }))
        .send()
        .context("aftermath /api/price-info HTTP")?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        bail!(
            "aftermath /api/price-info HTTP {status}: {}",
            truncate(&text, 180)
        );
    }
    let value: Value = serde_json::from_str(&text).context("aftermath /api/price-info JSON")?;
    let mut raw: HashMap<String, (f64, f64)> = HashMap::new();
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            let price = v
                .get("price")
                .and_then(|p| p.as_f64())
                .unwrap_or(0.0);
            let chg = v
                .get("priceChange24HoursPercentage")
                .and_then(|p| p.as_f64())
                .unwrap_or(0.0);
            if price.is_finite() && price > 0.0 {
                raw.insert(k.clone(), (price, chg));
            }
        }
    }

    let price_only: HashMap<String, f64> = raw.iter().map(|(k, (p, _))| (k.clone(), *p)).collect();
    let mut out = HashMap::new();
    for t in coin_types {
        if let Some(p) = lookup_price(&price_only, t) {
            let chg = raw
                .iter()
                .find(|(k, _)| {
                    k.eq_ignore_ascii_case(t)
                        || k.eq_ignore_ascii_case(&canonicalize_coin_type(t))
                })
                .map(|(_, (_, c))| *c)
                .unwrap_or(0.0);
            out.insert(t.clone(), (p, chg));
        }
    }
    Ok(out)
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_short_sui() {
        assert_eq!(canonicalize_coin_type("0x2::sui::SUI"), "0x2::sui::SUI");
        assert_eq!(canonicalize_coin_type("2::sui::SUI"), "0x2::sui::SUI");
    }
}
