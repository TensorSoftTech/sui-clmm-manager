//! Liquidity series for the detail chart (Sol `clmm_math::build_liquidity_series` port).

use crate::position::TickSample;
use std::collections::BTreeMap;

/// Which token(s) a graph bar represents for *this position*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSide {
    Outside,
    A,
    B,
    Both,
}

#[derive(Debug, Clone, Copy)]
pub struct LiquiditySample {
    pub tick: i32,
    pub liquidity: u64,
    pub side: TokenSide,
    pub in_position: bool,
}

pub const CHART_HALF_SPACINGS_DEFAULT: i32 = 10_000;
pub const CHART_HALF_SPACINGS_MIN: i32 = 4;
pub const CHART_FETCH_HALF_SPACINGS: i32 = 512;

#[derive(Debug, Clone, Copy)]
pub struct TickPoint {
    pub index: i32,
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
}

impl From<&TickSample> for TickPoint {
    fn from(t: &TickSample) -> Self {
        Self {
            index: t.index,
            liquidity_net: t.liquidity_net,
            liquidity_gross: t.liquidity_gross,
        }
    }
}

pub fn scale_u128(v: u128) -> u64 {
    if v == 0 {
        1
    } else if v <= u64::MAX as u128 {
        v as u64
    } else {
        ((v >> 32) as u64).max(1)
    }
}

/// Prefer liquidity_gross histogram; paint position range; tag A / B / Both.
pub fn build_liquidity_series(
    ticks: &BTreeMap<i32, TickPoint>,
    pool_liquidity: u128,
    position_liquidity: u128,
    current_tick: i32,
    lower: i32,
    upper: i32,
    spacing: u32,
    pad_spacings: i32,
) -> Vec<LiquiditySample> {
    let spacing = spacing.max(1);
    let pad = spacing as i32 * pad_spacings.max(2);
    let min_t = lower.min(current_tick) - pad;
    let max_t = upper.max(current_tick) + pad;
    if min_t >= max_t {
        return Vec::new();
    }

    let mut gross: BTreeMap<i32, u128> = BTreeMap::new();
    for t in ticks.values() {
        if t.index >= min_t && t.index <= max_t && t.liquidity_gross > 0 {
            gross.insert(t.index, t.liquidity_gross);
        }
    }

    let mut active: BTreeMap<i32, u128> = BTreeMap::new();
    active.insert(current_tick, pool_liquidity);

    let mut events: Vec<(i32, i128)> = ticks
        .values()
        .filter(|t| t.index >= min_t && t.index <= max_t)
        .map(|t| (t.index, t.liquidity_net))
        .collect();
    events.sort_by_key(|(i, _)| *i);

    let mut l = pool_liquidity as i128;
    for &(idx, net) in events.iter().rev().filter(|(i, _)| *i <= current_tick) {
        l = l.saturating_sub(net);
        active.insert(idx, l.max(0) as u128);
    }
    l = pool_liquidity as i128;
    for &(idx, net) in events.iter().filter(|(i, _)| *i > current_tick) {
        l = l.saturating_add(net);
        active.insert(idx, l.max(0) as u128);
    }

    let mut merged: BTreeMap<i32, u128> = active;
    for (k, v) in gross {
        merged
            .entry(k)
            .and_modify(|e| *e = (*e).max(v))
            .or_insert(v);
    }

    let pos_h = position_liquidity.max(1);
    if merged.len() <= 1 {
        // No tick table: paint a solid position band (A left of spot, B right).
        // Do NOT invent outside shelves at pool/8 — that dwarfs a small position and
        // leaves an empty gap next to the spot spike (what the TUI was showing).
        let span = (upper - lower).max(1);
        let step = (span / 48).max(spacing as i32).max(1);
        let mut t = lower;
        while t <= upper {
            merged.insert(t, pos_h);
            t = t.saturating_add(step);
        }
        merged.insert(lower, pos_h);
        merged.insert(upper, pos_h);
        // Extent pins so zoom/X axis still covers pad around spot + range.
        merged.entry(min_t).or_insert(1);
        merged.entry(max_t).or_insert(1);
        merged.insert(current_tick, pos_h);
    } else {
        for t in [lower, upper, current_tick] {
            merged.entry(t).or_insert(if t == current_tick {
                pool_liquidity.max(pos_h)
            } else {
                pos_h
            });
        }
    }

    let points: Vec<(i32, u128)> = merged.into_iter().collect();
    downsample(&points, 128, current_tick, lower, upper)
}

/// Visible `[lo, hi]` ticks (± half spacings around spot), clamped to series extent.
/// Always expands to cover `[range_lo, range_hi]` so OOR / right-side range stays visible.
pub fn chart_view_window(
    current: i32,
    spacing: u32,
    half_spacings: i32,
    extent: (i32, i32),
    range_lo: i32,
    range_hi: i32,
) -> (i32, i32) {
    let step = spacing.max(1) as i32;
    let half = half_spacings.max(CHART_HALF_SPACINGS_MIN) * step;
    let (smin, smax) = extent;
    let mut lo = current - half;
    let mut hi = current + half;
    lo = lo.min(range_lo).min(current);
    hi = hi.max(range_hi).max(current);
    lo = lo.max(smin);
    hi = hi.min(smax);
    if lo >= hi {
        (smin, smax)
    } else {
        (lo, hi)
    }
}

/// Filter samples to a tick window; pin `view_lo` / `view_hi` so X spans the zoom
/// even when edges have no depth (Sol twin).
pub fn slice_liquidity_series(
    series: &[LiquiditySample],
    view_lo: i32,
    view_hi: i32,
) -> (Vec<LiquiditySample>, u64) {
    if series.is_empty() {
        return (Vec::new(), 1);
    }
    let mut out: Vec<LiquiditySample> = series
        .iter()
        .copied()
        .filter(|s| s.tick >= view_lo && s.tick <= view_hi)
        .collect();
    if out.is_empty() {
        out = series.to_vec();
    }
    let edge = |tick: i32| LiquiditySample {
        tick,
        liquidity: 1,
        side: TokenSide::Outside,
        in_position: false,
    };
    if out.first().is_none_or(|s| s.tick > view_lo) {
        out.insert(0, edge(view_lo));
    }
    if out.last().is_none_or(|s| s.tick < view_hi) {
        out.push(edge(view_hi));
    }
    let max_l = out
        .iter()
        .filter(|s| s.liquidity > 1)
        .map(|s| s.liquidity)
        .max()
        .unwrap_or(1)
        .max(1);
    (out, max_l)
}

fn classify_side(tick: i32, current: i32, lower: i32, upper: i32) -> (TokenSide, bool) {
    let in_position = tick >= lower && tick <= upper;
    if !in_position {
        return (TokenSide::Outside, false);
    }
    let side = if tick < current {
        TokenSide::A
    } else if tick > current {
        TokenSide::B
    } else {
        TokenSide::Both
    };
    (side, true)
}

fn downsample(
    points: &[(i32, u128)],
    buckets: usize,
    current: i32,
    lower: i32,
    upper: i32,
) -> Vec<LiquiditySample> {
    if points.is_empty() {
        return Vec::new();
    }
    let map_one = |t: i32, l: u128| {
        let (side, in_position) = classify_side(t, current, lower, upper);
        LiquiditySample {
            tick: t,
            liquidity: scale_u128(l.max(1)),
            side,
            in_position,
        }
    };

    if points.len() <= buckets {
        return points.iter().map(|(t, l)| map_one(*t, *l)).collect();
    }
    let min_t = points.first().unwrap().0;
    let max_t = points.last().unwrap().0;
    let span = (max_t - min_t).max(1) as f64;
    let mut out = Vec::with_capacity(buckets);
    for b in 0..buckets {
        let edge0 = min_t + ((b as f64 / buckets as f64) * span) as i32;
        let edge1 = min_t + (((b + 1) as f64 / buckets as f64) * span) as i32;
        let mut max_l = 0u128;
        let mut mid = (edge0 + edge1) / 2;
        for &(t, l) in points {
            if t >= edge0 && t < edge1.max(edge0 + 1) {
                if l > max_l {
                    max_l = l;
                    mid = t;
                }
            }
        }
        if max_l == 0 {
            continue;
        }
        out.push(map_one(mid, max_l));
    }
    // Ensure current / bounds survive downsample.
    for &t in &[current, lower, upper] {
        if !out.iter().any(|s| s.tick == t)
            && let Some(&(pt, l)) = points.iter().min_by_key(|(x, _)| (*x - t).abs())
        {
            out.push(map_one(pt, l));
        }
    }
    out.sort_by_key(|s| s.tick);
    out.dedup_by_key(|s| s.tick);
    out
}

/// Build chart series from raw tick samples (+ synthetic range if empty).
pub fn series_from_ticks(
    ticks: &[TickSample],
    pool_liquidity: u128,
    position_liquidity: u128,
    current_tick: i32,
    lower: i32,
    upper: i32,
    spacing: u32,
) -> (Vec<LiquiditySample>, u64, i32, i32) {
    let map: BTreeMap<i32, TickPoint> = ticks
        .iter()
        .map(|t| (t.index, TickPoint::from(t)))
        .collect();
    let series = build_liquidity_series(
        &map,
        pool_liquidity,
        position_liquidity,
        current_tick,
        lower,
        upper,
        spacing,
        CHART_FETCH_HALF_SPACINGS.min(64),
    );
    let min_t = series.first().map(|s| s.tick).unwrap_or(lower.min(current_tick));
    let max_t = series.last().map(|s| s.tick).unwrap_or(upper.max(current_tick));
    let max_l = series
        .iter()
        .filter(|s| s.in_position)
        .map(|s| s.liquidity)
        .max()
        .or_else(|| series.iter().map(|s| s.liquidity).max())
        .unwrap_or(1)
        .max(1);
    (series, max_l, min_t, max_t)
}
