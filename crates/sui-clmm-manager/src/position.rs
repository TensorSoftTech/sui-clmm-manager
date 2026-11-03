//! Shared LP position list / detail types (wallet-owned).

use crate::Venue;

/// One concentrated-liquidity position row (list view).
#[derive(Debug, Clone)]
pub struct PositionRow {
    pub venue: Venue,
    pub object_id: String,
    pub pool_id: String,
    pub pool_label: String,
    pub coin_type_a: String,
    pub coin_type_b: String,
    pub symbol_a: String,
    pub symbol_b: String,
    pub decimals_a: u8,
    pub decimals_b: u8,
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub liquidity: u128,
    pub fee_rate: u32,
    /// Fees owed if encoded on the position (0 if unknown / needs claim-sim).
    pub fee_owed_a: u128,
    pub fee_owed_b: u128,
    /// Filled on detail / enrich when pool is fetched.
    pub current_tick: Option<i32>,
    pub tick_spacing: Option<u32>,
    pub in_range: Option<bool>,
    /// Turbos: underlying position object (NFT points here). Empty for others.
    pub inner_position_id: String,
}

impl PositionRow {
    pub fn pair_label(&self) -> String {
        if !self.symbol_a.is_empty() && !self.symbol_b.is_empty() {
            format!("{}/{}", self.symbol_a, self.symbol_b)
        } else if !self.pool_label.is_empty() {
            self.pool_label.clone()
        } else {
            crate::pool::short_id(&self.pool_id)
        }
    }

    pub fn range_label(&self) -> String {
        format!("[{}, {}]", self.tick_lower, self.tick_upper)
    }

    pub fn liquidity_label(&self) -> String {
        crate::pool::format_compact_u128(self.liquidity)
    }

    pub fn fee_bps_label(&self) -> String {
        if self.fee_rate == 0 {
            return "—".into();
        }
        let bps = self.fee_rate as f64 / 100.0;
        if bps >= 10.0 {
            format!("{bps:.0} bps")
        } else {
            format!("{bps:.1} bps")
        }
    }

    pub fn status_label(&self) -> &'static str {
        match self.in_range {
            Some(true) => "in",
            Some(false) => "out",
            None => "?",
        }
    }
}

/// Tick sample for detail / future chart (pool tick table entries).
#[derive(Debug, Clone)]
pub struct TickSample {
    pub index: i32,
    pub liquidity_gross: u128,
    pub liquidity_net: i128,
    pub fee_growth_outside_a: u128,
    pub fee_growth_outside_b: u128,
    /// Per-reward-index outside growth (Bluefin `reward_growths_outside`).
    pub reward_growth_outside: Vec<u128>,
}

/// One claimable reward stream on a position.
#[derive(Debug, Clone)]
pub struct RewardLine {
    pub coin_type: String,
    pub symbol: String,
    pub decimals: u8,
    pub amount: u128,
    pub amount_ui: String,
    pub usd: Option<f64>,
    /// True when growth + tick outsides were used (not just stored owed).
    pub claim_ok: bool,
}

/// Position detail with pool snapshot + optional nearby ticks + chart series.
#[derive(Debug, Clone)]
pub struct PositionDetail {
    pub row: PositionRow,
    pub pool_liquidity: u128,
    pub sqrt_price: u128,
    pub current_tick: i32,
    pub tick_spacing: u32,
    pub coin_a_reserve: u128,
    pub coin_b_reserve: u128,
    pub is_paused: bool,
    pub ticks: Vec<TickSample>,
    /// Token amounts currently in the position (from L + √P).
    pub amount_a: u128,
    pub amount_b: u128,
    pub amount_a_ui: String,
    pub amount_b_ui: String,
    /// Claim-simulated fees (stored owed + growth since checkpoint when possible).
    pub fee_a: u128,
    pub fee_b: u128,
    pub fee_a_ui: String,
    pub fee_b_ui: String,
    pub fees_claim_ok: bool,
    /// Claimable emissions (Bluefin / Cetus / …).
    pub rewards: Vec<RewardLine>,
    pub rewards_usd: Option<f64>,
    /// Chart series (position range painted over pool depth).
    pub series: Vec<crate::chart::LiquiditySample>,
    pub series_max_liquidity: u64,
    pub series_min_tick: i32,
    pub series_max_tick: i32,
    /// Aftermath (or later) USD quote for position / fees / vault.
    pub usd: Option<crate::pricing::UsdEstimate>,
    pub note: String,
}

/// Build a PositionDetail skeleton (venues fill fields).
pub fn detail_from_row(
    row: PositionRow,
    current_tick: i32,
    tick_spacing: u32,
    pool_liquidity: u128,
    sqrt_price: u128,
) -> PositionDetail {
    let mut row = row;
    row.current_tick = Some(current_tick);
    row.tick_spacing = Some(tick_spacing);
    row.in_range = Some(current_tick >= row.tick_lower && current_tick < row.tick_upper);
    let (amount_a, amount_b) = crate::clmm_math::amounts_from_liquidity(
        row.liquidity,
        sqrt_price,
        row.tick_lower,
        row.tick_upper,
    );
    let amount_a_ui = format!(
        "{} {}",
        crate::pool::format_amount(amount_a, row.decimals_a),
        row.symbol_a
    );
    let amount_b_ui = format!(
        "{} {}",
        crate::pool::format_amount(amount_b, row.decimals_b),
        row.symbol_b
    );
    let (series, series_max_liquidity, series_min_tick, series_max_tick) =
        crate::chart::series_from_ticks(
            &[],
            pool_liquidity,
            row.liquidity,
            current_tick,
            row.tick_lower,
            row.tick_upper,
            tick_spacing,
        );
    let fee_a = row.fee_owed_a;
    let fee_b = row.fee_owed_b;
    let fee_a_ui = format!(
        "{} {}",
        crate::pool::format_amount(fee_a, row.decimals_a),
        row.symbol_a
    );
    let fee_b_ui = format!(
        "{} {}",
        crate::pool::format_amount(fee_b, row.decimals_b),
        row.symbol_b
    );
    PositionDetail {
        row,
        pool_liquidity,
        sqrt_price,
        current_tick,
        tick_spacing,
        coin_a_reserve: 0,
        coin_b_reserve: 0,
        is_paused: false,
        ticks: Vec::new(),
        amount_a,
        amount_b,
        amount_a_ui,
        amount_b_ui,
        fee_a,
        fee_b,
        fee_a_ui,
        fee_b_ui,
        fees_claim_ok: false,
        rewards: Vec::new(),
        rewards_usd: None,
        series,
        series_max_liquidity,
        series_min_tick,
        series_max_tick,
        usd: None,
        note: String::new(),
    }
}

impl PositionDetail {
    /// Attach Aftermath USD prices for A/B (amounts, fees, vault) + reward coins.
    pub fn attach_usd(&mut self, mode: crate::pricing::PriceMode) {
        if mode == crate::pricing::PriceMode::Off {
            self.usd = None;
            self.rewards_usd = None;
            for r in &mut self.rewards {
                r.usd = None;
            }
            return;
        }
        let mut coins = vec![
            self.row.coin_type_a.clone(),
            self.row.coin_type_b.clone(),
        ];
        for r in &self.rewards {
            if !r.coin_type.is_empty() {
                coins.push(r.coin_type.clone());
            }
        }
        let prices = match crate::pricing::fetch_coin_usd_prices(mode, &coins) {
            Ok(m) => m,
            Err(e) => {
                self.usd = Some(crate::pricing::UsdEstimate {
                    mode,
                    note: format!("aftermath err · {e}"),
                    ..Default::default()
                });
                return;
            }
        };
        self.usd = Some(crate::pricing::estimate_position_usd(
            mode,
            &self.row.coin_type_a,
            &self.row.coin_type_b,
            self.row.decimals_a,
            self.row.decimals_b,
            self.amount_a,
            self.amount_b,
            self.fee_a,
            self.fee_b,
            self.coin_a_reserve,
            self.coin_b_reserve,
            &prices,
        ));
        let mut rewards_usd = 0.0;
        let mut any = false;
        for r in &mut self.rewards {
            let p = prices.get(&r.coin_type).copied().or_else(|| {
                prices
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(&r.coin_type))
                    .map(|(_, v)| *v)
            });
            r.usd = crate::pricing::raw_to_usd(r.amount, r.decimals, p);
            if let Some(u) = r.usd {
                rewards_usd += u;
                any = true;
            }
        }
        self.rewards_usd = any.then_some(rewards_usd);
    }

    /// Apply Uniswap-V3 claim-sim when tick outside + pool global growth are known.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_fee_quote(
        &mut self,
        growth_global_a: u128,
        growth_global_b: u128,
        checkpoint_a: u128,
        checkpoint_b: u128,
        outside_lower_a: u128,
        outside_lower_b: u128,
        outside_upper_a: u128,
        outside_upper_b: u128,
    ) {
        self.fee_a = crate::fees::collect_fees_quote(
            self.row.fee_owed_a,
            growth_global_a,
            checkpoint_a,
            outside_lower_a,
            outside_upper_a,
            self.row.tick_lower,
            self.row.tick_upper,
            self.current_tick,
            self.row.liquidity,
        );
        self.fee_b = crate::fees::collect_fees_quote(
            self.row.fee_owed_b,
            growth_global_b,
            checkpoint_b,
            outside_lower_b,
            outside_upper_b,
            self.row.tick_lower,
            self.row.tick_upper,
            self.current_tick,
            self.row.liquidity,
        );
        self.fees_claim_ok = true;
        self.fee_a_ui = format!(
            "{} {}",
            crate::pool::format_amount(self.fee_a, self.row.decimals_a),
            self.row.symbol_a
        );
        self.fee_b_ui = format!(
            "{} {}",
            crate::pool::format_amount(self.fee_b, self.row.decimals_b),
            self.row.symbol_b
        );
    }

    /// Rebuild size + chart after ticks / symbols are filled.
    pub fn rebuild_series(&mut self) {
        let (amount_a, amount_b) = crate::clmm_math::amounts_from_liquidity(
            self.row.liquidity,
            self.sqrt_price,
            self.row.tick_lower,
            self.row.tick_upper,
        );
        self.amount_a = amount_a;
        self.amount_b = amount_b;
        self.amount_a_ui = format!(
            "{} {}",
            crate::pool::format_amount(amount_a, self.row.decimals_a),
            self.row.symbol_a
        );
        self.amount_b_ui = format!(
            "{} {}",
            crate::pool::format_amount(amount_b, self.row.decimals_b),
            self.row.symbol_b
        );
        self.fee_a_ui = format!(
            "{} {}",
            crate::pool::format_amount(self.fee_a, self.row.decimals_a),
            self.row.symbol_a
        );
        self.fee_b_ui = format!(
            "{} {}",
            crate::pool::format_amount(self.fee_b, self.row.decimals_b),
            self.row.symbol_b
        );
        let (series, max_l, min_t, max_t) = crate::chart::series_from_ticks(
            &self.ticks,
            self.pool_liquidity,
            self.row.liquidity,
            self.current_tick,
            self.row.tick_lower,
            self.row.tick_upper,
            self.tick_spacing,
        );
        self.series = series;
        self.series_max_liquidity = max_l;
        self.series_min_tick = min_t;
        self.series_max_tick = max_t;
    }
}

/// One saved position size snapshot (SQLite history).
#[derive(Debug, Clone)]
pub struct PositionSnapshotRow {
    pub id: u64,
    pub created_at: i64,
    pub venue: String,
    pub object_id: String,
    pub pool_id: String,
    pub pair_label: String,
    pub liquidity: u128,
    pub amount_a: u128,
    pub amount_b: u128,
    pub symbol_a: String,
    pub symbol_b: String,
    pub decimals_a: u8,
    pub decimals_b: u8,
    pub current_tick: i32,
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub in_range: bool,
}

impl PositionSnapshotRow {
    pub fn time_label(&self) -> String {
        let secs = self.created_at.max(0) as u64;
        let days = (secs / 86_400) as i64;
        let tod = secs % 86_400;
        let h = tod / 3600;
        let m = (tod % 3600) / 60;
        let (y, mo, d) = unix_days_to_ymd(days);
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}")
    }

    pub fn size_label(&self) -> String {
        format!(
            "{} {} + {} {}",
            crate::pool::format_amount(self.amount_a, self.decimals_a),
            self.symbol_a,
            crate::pool::format_amount(self.amount_b, self.decimals_b),
            self.symbol_b
        )
    }

    pub fn delta_vs(&self, prev: &Self) -> String {
        let da = self.amount_a as i128 - prev.amount_a as i128;
        let db = self.amount_b as i128 - prev.amount_b as i128;
        let fa = format_signed_amount(da, self.decimals_a, &self.symbol_a);
        let fb = format_signed_amount(db, self.decimals_b, &self.symbol_b);
        format!("{fa} · {fb}")
    }
}

fn format_signed_amount(delta: i128, decimals: u8, sym: &str) -> String {
    let sign = if delta < 0 { "−" } else if delta > 0 { "+" } else { "" };
    let abs = delta.unsigned_abs();
    format!(
        "{sign}{} {sym}",
        crate::pool::format_amount(abs, decimals)
    )
}

fn unix_days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Civil from days (Howard Hinnant) — no chrono dep.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}
