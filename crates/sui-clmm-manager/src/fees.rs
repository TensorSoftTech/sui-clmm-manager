//! Uniswap-V3-style fee claim simulation (integer Q64.64).

use crate::clmm_math::mul_shift_right_64;

/// `fee_growth_inside = global − below − above` (wrapping).
pub fn fee_growth_inside(
    global: u128,
    outside_lower: u128,
    outside_upper: u128,
    tick_lower: i32,
    tick_upper: i32,
    tick_current: i32,
) -> u128 {
    let mut below = outside_lower;
    let mut above = outside_upper;
    if tick_current < tick_lower {
        below = global.wrapping_sub(outside_lower);
    }
    if tick_current >= tick_upper {
        above = global.wrapping_sub(outside_upper);
    }
    global.wrapping_sub(below).wrapping_sub(above)
}

/// `(growth_inside − checkpoint) * liquidity >> 64` as u128.
pub fn tokens_owed_delta(growth_inside: u128, checkpoint: u128, liquidity: u128) -> u128 {
    let delta = growth_inside.wrapping_sub(checkpoint);
    mul_shift_right_64(delta, liquidity)
}

/// Bluefin `update_reward_infos`: accrue global growth for elapsed emission time.
///
/// `growth += (reward_per_seconds * dt) / pool_liquidity` where `dt` is clipped
/// to `[last_update, ended_at]` (same as on-chain before collect).
pub fn accrue_reward_growth_global(
    growth_global: u128,
    reward_per_seconds: u128,
    last_update_time: u64,
    ended_at_seconds: u64,
    now_seconds: u64,
    pool_liquidity: u128,
) -> u128 {
    if pool_liquidity == 0 || reward_per_seconds == 0 {
        return growth_global;
    }
    let end = now_seconds.min(ended_at_seconds);
    if end <= last_update_time {
        return growth_global;
    }
    let dt = u128::from(end - last_update_time);
    // full_mul(dt, rps) / L  — use u128 saturating path (dt*rps fits for live pools).
    let accumulated = dt.saturating_mul(reward_per_seconds) / pool_liquidity;
    growth_global.wrapping_add(accumulated)
}

/// Claimable = stored owed + growth delta since checkpoint.
#[allow(clippy::too_many_arguments)]
pub fn collect_fees_quote(
    fee_owed: u128,
    growth_global: u128,
    growth_checkpoint: u128,
    outside_lower: u128,
    outside_upper: u128,
    tick_lower: i32,
    tick_upper: i32,
    tick_current: i32,
    liquidity: u128,
) -> u128 {
    let inside = fee_growth_inside(
        growth_global,
        outside_lower,
        outside_upper,
        tick_lower,
        tick_upper,
        tick_current,
    );
    let delta = tokens_owed_delta(inside, growth_checkpoint, liquidity);
    fee_owed.saturating_add(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_when_current_in_range() {
        // current in [lower, upper): below=out_lo, above=out_hi
        let g = 1000u128;
        let inside = fee_growth_inside(g, 100, 200, -10, 10, 0);
        assert_eq!(inside, g.wrapping_sub(100).wrapping_sub(200));
    }

    #[test]
    fn accrue_clips_to_ended_at() {
        let g = 100u128;
        let rps = 1_000u128;
        // ended before now → only accrue until ended
        let out = accrue_reward_growth_global(g, rps, 10, 20, 50, 100);
        assert_eq!(out, g + (rps * 10) / 100);
    }

    #[test]
    fn accrue_noop_when_ended() {
        let g = 100u128;
        let out = accrue_reward_growth_global(g, 999, 100, 50, 200, 1);
        assert_eq!(out, g);
    }
}
