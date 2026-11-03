//! CLMM amounts from L + √P (Q64.64) — Sol / Cetus / Bluefin-compatible.

const MIN_TICK: i32 = -443_636;
const MAX_TICK: i32 = 443_636;
const Q64: u128 = 1u128 << 64;

/// Estimate (amount_a, amount_b) in raw token units from L + √P + tick range.
pub fn amounts_from_liquidity(
    liquidity: u128,
    sqrt_price_x64: u128,
    tick_lower: i32,
    tick_upper: i32,
) -> (u128, u128) {
    if liquidity == 0 || tick_lower >= tick_upper {
        return (0, 0);
    }

    let Some(sa) = get_sqrt_price_at_tick(tick_lower) else {
        return (0, 0);
    };
    let Some(sb) = get_sqrt_price_at_tick(tick_upper) else {
        return (0, 0);
    };
    let sp = sqrt_price_x64;
    if sa == 0 || sb <= sa || sp == 0 {
        return (0, 0);
    }

    if sp <= sa {
        (get_amount_0_delta(sa, sb, liquidity), 0)
    } else if sp >= sb {
        (0, get_amount_1_delta(sa, sb, liquidity))
    } else {
        (
            get_amount_0_delta(sp, sb, liquidity),
            get_amount_1_delta(sa, sp, liquidity),
        )
    }
}

/// Q64.64 √P at tick — Uniswap V3 / Cetus / Bluefin bit product.
pub fn get_sqrt_price_at_tick(tick: i32) -> Option<u128> {
    if !(MIN_TICK..=MAX_TICK).contains(&tick) {
        return None;
    }
    let abs_tick = tick.unsigned_abs();

    let mut ratio: u128 = if abs_tick & 0x1 != 0 {
        0xfffcb933bd6fb800
    } else {
        Q64
    };

    const MAG: [(u32, u128); 18] = [
        (0x2, 0xfff97272373d4000),
        (0x4, 0xfff2e50f5f657000),
        (0x8, 0xffe5caca7e10f000),
        (0x10, 0xffcb9843d60f7000),
        (0x20, 0xff973b41fa98e800),
        (0x40, 0xff2ea16466c9b000),
        (0x80, 0xfe5dee046a9a3800),
        (0x100, 0xfcbe86c7900bb000),
        (0x200, 0xf987a7253ac65800),
        (0x400, 0xf3392b0822bb6000),
        (0x800, 0xe7159475a2caf000),
        (0x1000, 0xd097f3bdfd2f2000),
        (0x2000, 0xa9f746462d9f8000),
        (0x4000, 0x70d869a156f31c00),
        (0x8000, 0x31be135f97ed3200),
        (0x10000, 0x9aa508b5b85a500),
        (0x20000, 0x5d6af8dedc582c),
        (0x40000, 0x2216e584f5fa),
    ];
    for &(bit, mag) in &MAG {
        if abs_tick & bit != 0 {
            ratio = mul_shift_right_64(ratio, mag);
        }
    }

    if tick > 0 {
        if ratio == 0 {
            return None;
        }
        ratio = u128::MAX / ratio;
    }
    Some(ratio)
}

fn get_amount_0_delta(mut sa: u128, mut sb: u128, liquidity: u128) -> u128 {
    if sa > sb {
        std::mem::swap(&mut sa, &mut sb);
    }
    if sa == 0 {
        return 0;
    }
    div_shl_64(liquidity, sa).saturating_sub(div_shl_64(liquidity, sb))
}

fn get_amount_1_delta(mut sa: u128, mut sb: u128, liquidity: u128) -> u128 {
    if sa > sb {
        std::mem::swap(&mut sa, &mut sb);
    }
    mul_shift_right_64(liquidity, sb - sa)
}

/// Floor((a * b) / 2^64).
pub fn mul_shift_right_64(a: u128, b: u128) -> u128 {
    let a_lo = a as u64 as u128;
    let a_hi = a >> 64;
    let b_lo = b as u64 as u128;
    let b_hi = b >> 64;

    let lo_lo = a_lo * b_lo;
    let (mid, c1) = (a_lo * b_hi).overflowing_add(a_hi * b_lo);
    let mut hi = a_hi * b_hi;
    if c1 {
        hi = hi.saturating_add(1 << 64);
    }
    let (mid2, c2) = mid.overflowing_add(lo_lo >> 64);
    if c2 {
        hi = hi.saturating_add(1);
    }
    if hi > u64::MAX as u128 {
        return u128::MAX;
    }
    hi.saturating_mul(1u128 << 64).saturating_add(mid2)
}

fn div_shl_64(n: u128, d: u128) -> u128 {
    if d == 0 {
        return 0;
    }
    let n_hi = n >> 64;
    let n_lo = n << 64;
    div_256_by_u128(n_hi, n_lo, d)
}

fn div_256_by_u128(n_hi: u128, n_lo: u128, d: u128) -> u128 {
    if d == 0 {
        return 0;
    }
    if n_hi == 0 {
        return n_lo / d;
    }
    let q_hi = n_hi / d;
    let rem = n_hi % d;
    let mid = (rem << 64) | (n_lo >> 64);
    let q_mid = mid / d;
    let rem2 = mid % d;
    let low = (rem2 << 64) | (n_lo as u64 as u128);
    let q_lo = low / d;
    if q_hi > 0 {
        return u128::MAX;
    }
    q_mid.saturating_mul(Q64).saturating_add(q_lo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqrt_at_zero() {
        assert_eq!(get_sqrt_price_at_tick(0), Some(Q64));
    }

    #[test]
    fn amounts_in_range_nonzero() {
        let sp = get_sqrt_price_at_tick(0).unwrap();
        let (a, b) = amounts_from_liquidity(1_000_000, sp, -100, 100);
        assert!(a > 0 || b > 0);
    }
}
