//! USD estimates from Aftermath prices (Sol twin `pricing` subset).

use crate::quotes::aftermath;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PriceMode {
    #[default]
    Aftermath,
    Off,
}

impl PriceMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Aftermath => "aftermath",
            Self::Off => "off",
        }
    }

    pub fn from_setting(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "0" => Self::Off,
            _ => Self::Aftermath,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Aftermath => Self::Off,
            Self::Off => Self::Aftermath,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsd {
    pub price: Option<f64>,
    pub source: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct UsdEstimate {
    pub mode: PriceMode,
    pub price_a: TokenUsd,
    pub price_b: TokenUsd,
    pub amount_a_usd: Option<f64>,
    pub amount_b_usd: Option<f64>,
    pub fees_a_usd: Option<f64>,
    pub fees_b_usd: Option<f64>,
    pub position_usd: Option<f64>,
    pub fees_usd: Option<f64>,
    pub pool_vault_a_usd: Option<f64>,
    pub pool_vault_b_usd: Option<f64>,
    pub pool_tvl_usd: Option<f64>,
    pub note: String,
}

pub fn format_usd(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let abs = v.abs();
    if abs >= 1_000_000.0 {
        format!("${:.2}M", v / 1_000_000.0)
    } else if abs >= 10_000.0 {
        format!("${:.0}", v)
    } else if abs >= 100.0 {
        format!("${:.1}", v)
    } else if abs >= 1.0 {
        format!("${:.2}", v)
    } else if abs >= 0.01 {
        format!("${:.4}", v)
    } else if abs > 0.0 {
        format!("${:.6}", v)
    } else {
        "$0".into()
    }
}

/// Compact axis labels for the liquidity chart Y scale.
pub fn format_usd_axis(x: f64) -> String {
    if !x.is_finite() || x < 0.0 {
        return "—".into();
    }
    if x >= 1_000_000.0 {
        format!("${:.1}M", x / 1_000_000.0)
    } else if x >= 10_000.0 {
        format!("${:.0}k", x / 1_000.0)
    } else if x >= 1000.0 {
        format!("${:.1}k", x / 1_000.0)
    } else if x >= 1.0 {
        format!("${x:.2}")
    } else if x >= 0.01 {
        format!("${x:.4}")
    } else if x == 0.0 {
        "$0".into()
    } else {
        format!("${x:.2e}")
    }
}

/// Map a series liquidity sample to ≈ USD using pool active L ↔ vault TVL.
pub fn liquidity_l_to_usd(l: u64, pool_liquidity: u128, pool_tvl_usd: f64) -> Option<f64> {
    if pool_liquidity == 0 || !pool_tvl_usd.is_finite() || pool_tvl_usd < 0.0 {
        return None;
    }
    Some(pool_tvl_usd * (l as f64 / pool_liquidity as f64))
}

pub fn raw_to_usd(raw: u128, decimals: u8, price: Option<f64>) -> Option<f64> {
    let p = price?;
    if !p.is_finite() || p <= 0.0 {
        return None;
    }
    let ui = if decimals == 0 {
        raw as f64
    } else {
        raw as f64 / 10f64.powi(i32::from(decimals))
    };
    let usd = ui * p;
    usd.is_finite().then_some(usd)
}

/// Fetch Aftermath USD map for the given coin types (no-op when mode is Off).
pub fn fetch_coin_usd_prices(
    mode: PriceMode,
    coin_types: &[String],
) -> Result<HashMap<String, f64>, String> {
    match mode {
        PriceMode::Off => Ok(HashMap::new()),
        PriceMode::Aftermath => aftermath::fetch_usd_prices(coin_types).map_err(|e| e.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn estimate_position_usd(
    mode: PriceMode,
    coin_a: &str,
    coin_b: &str,
    decimals_a: u8,
    decimals_b: u8,
    amount_a: u128,
    amount_b: u128,
    fee_a: u128,
    fee_b: u128,
    vault_a: u128,
    vault_b: u128,
    prices: &HashMap<String, f64>,
) -> UsdEstimate {
    let pa = prices.get(coin_a).copied().or_else(|| {
        prices
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(coin_a))
            .map(|(_, v)| *v)
    });
    let pb = prices.get(coin_b).copied().or_else(|| {
        prices
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(coin_b))
            .map(|(_, v)| *v)
    });

    let source = match mode {
        PriceMode::Aftermath => "aftermath",
        PriceMode::Off => "off",
    };
    let amount_a_usd = raw_to_usd(amount_a, decimals_a, pa);
    let amount_b_usd = raw_to_usd(amount_b, decimals_b, pb);
    let fees_a_usd = raw_to_usd(fee_a, decimals_a, pa);
    let fees_b_usd = raw_to_usd(fee_b, decimals_b, pb);
    let pool_vault_a_usd = raw_to_usd(vault_a, decimals_a, pa);
    let pool_vault_b_usd = raw_to_usd(vault_b, decimals_b, pb);

    let position_usd = match (amount_a_usd, amount_b_usd) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        _ => None,
    };
    let fees_usd = match (fees_a_usd, fees_b_usd) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        _ => None,
    };
    let pool_tvl_usd = match (pool_vault_a_usd, pool_vault_b_usd) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        _ => None,
    };

    let note = match mode {
        PriceMode::Off => "usd off".into(),
        PriceMode::Aftermath if pa.is_some() || pb.is_some() => {
            format!(
                "aftermath · {}={} · {}={}",
                "A",
                pa.map(format_usd).unwrap_or_else(|| "—".into()),
                "B",
                pb.map(format_usd).unwrap_or_else(|| "—".into()),
            )
        }
        PriceMode::Aftermath => "aftermath · no price".into(),
    };

    UsdEstimate {
        mode,
        price_a: TokenUsd {
            price: pa,
            source,
        },
        price_b: TokenUsd {
            price: pb,
            source,
        },
        amount_a_usd,
        amount_b_usd,
        fees_a_usd,
        fees_b_usd,
        position_usd,
        fees_usd,
        pool_vault_a_usd,
        pool_vault_b_usd,
        pool_tvl_usd,
        note,
    }
}
