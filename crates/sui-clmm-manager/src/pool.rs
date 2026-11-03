//! Shared pool list / detail types (pools-first).

use crate::Venue;

/// One pool row for the list view (prefetched top-N by liquidity).
#[derive(Debug, Clone)]
pub struct PoolRow {
    pub venue: Venue,
    pub object_id: String,
    pub name: String,
    pub coin_type_a: String,
    pub coin_type_b: String,
    pub symbol_a: String,
    pub symbol_b: String,
    pub decimals_a: u8,
    pub decimals_b: u8,
    pub fee_rate: u32,
    pub tick_spacing: u32,
    pub liquidity: u128,
    pub coin_a: u128,
    pub coin_b: u128,
    pub current_tick: i32,
    pub sqrt_price: u128,
    pub is_paused: bool,
    /// Rank after sort (1-based); 0 if unknown.
    pub rank: u32,
}

impl PoolRow {
    pub fn pair_label(&self) -> String {
        if !self.symbol_a.is_empty() && !self.symbol_b.is_empty() {
            format!("{}/{}", self.symbol_a, self.symbol_b)
        } else if !self.name.is_empty() {
            self.name.clone()
        } else {
            short_id(&self.object_id)
        }
    }

    pub fn fee_bps_label(&self) -> String {
        // Bluefin fee_rate is in 1e6 units (10000 = 1%).
        let bps = self.fee_rate as f64 / 100.0;
        if bps >= 10.0 {
            format!("{bps:.0} bps")
        } else {
            format!("{bps:.1} bps")
        }
    }

    pub fn liquidity_label(&self) -> String {
        format_compact_u128(self.liquidity)
    }
}

/// Thin pool detail for the detail screen.
#[derive(Debug, Clone)]
pub struct PoolDetail {
    pub row: PoolRow,
    pub fee_growth_global_a: u128,
    pub fee_growth_global_b: u128,
    pub protocol_fee_a: u128,
    pub protocol_fee_b: u128,
    pub protocol_fee_share: u32,
    pub position_index: u64,
    pub sequence_number: u64,
    pub icon_url: String,
}

/// Result of a top-N prefetch into SQLite.
#[derive(Debug, Clone, Default)]
pub struct PrefetchStats {
    pub fetched: usize,
    pub stored: usize,
    pub venues: Vec<(Venue, usize)>,
    pub graphql_calls: u64,
}

pub fn short_id(id: &str) -> String {
    let s = id.trim_start_matches("0x");
    if s.len() <= 10 {
        id.to_string()
    } else {
        format!("0x{}…{}", &s[..4], &s[s.len().saturating_sub(4)..])
    }
}

pub fn format_compact_u128(n: u128) -> String {
    if n >= 1_000_000_000_000 {
        format!("{:.1}T", n as f64 / 1e12)
    } else if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

pub fn format_amount(raw: u128, decimals: u8) -> String {
    if decimals == 0 {
        return raw.to_string();
    }
    let scale = 10u128.pow(u32::from(decimals));
    let whole = raw / scale;
    let frac = raw % scale;
    let frac_str = format!("{frac:0width$}", width = decimals as usize);
    let frac_trim = frac_str.trim_end_matches('0');
    if frac_trim.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{frac_trim}")
    }
}

/// Parse `Package::module::Type<A,B>` type params into (A, B).
pub fn parse_pool_type_params(type_repr: &str) -> Option<(String, String)> {
    let start = type_repr.find('<')?;
    let end = type_repr.rfind('>')?;
    let inner = &type_repr[start + 1..end];
    // Split on top-level comma (type params themselves may contain `<>`).
    let mut depth = 0i32;
    let mut split_at = None;
    for (i, ch) in inner.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                split_at = Some(i);
                break;
            }
            _ => {}
        }
    }
    let mid = split_at?;
    let a = inner[..mid].trim().to_string();
    let b = inner[mid + 1..].trim().to_string();
    Some((a, b))
}

/// Last path segment of a Move type as a crude symbol (`sui::SUI` → `SUI`).
pub fn crude_symbol(coin_type: &str) -> String {
    coin_type
        .rsplit("::")
        .next()
        .unwrap_or(coin_type)
        .to_string()
}

/// IntegerMate `I32 { bits }` → signed tick.
pub fn i32_from_bits(bits: u32) -> i32 {
    bits as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_type_params() {
        let t = "0xabc::pool::Pool<0x2::sui::SUI,0x1::usdc::USDC>";
        let (a, b) = parse_pool_type_params(t).unwrap();
        assert_eq!(a, "0x2::sui::SUI");
        assert_eq!(b, "0x1::usdc::USDC");
    }
}
