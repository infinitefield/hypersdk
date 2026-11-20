use std::str::FromStr;

use alloy::primitives::U256;
use rust_decimal::{Decimal, RoundingStrategy};

/// Converts a decimal amount to wei representation.
///
/// Wei is the smallest unit of Ethereum tokens (like satoshis for Bitcoin).
///
/// # Parameters
///
/// - `size`: The decimal amount to convert
/// - `decimals`: Number of decimal places for the token (e.g., 18 for ETH, 6 for USDC)
///
/// # Example
///
/// Convert 1.5 ETH to wei (18 decimals): `to_wei(dec!(1.5), 18)`
///
/// Digits past `decimals` are rounded half away from zero.
///
/// # Panics
///
/// Panics if `size` is negative, or if the result does not fit in a `U256`.
#[must_use]
#[inline]
pub fn to_wei(size: Decimal, decimals: u32) -> U256 {
    assert!(
        !size.is_sign_negative() || size.is_zero(),
        "to_wei: negative amount {size}"
    );
    // `Decimal::rescale` cannot grow the scale past what its 96-bit mantissa holds and
    // silently stops early, which made `to_wei(dec!(100_000_000_000), 18)` return a
    // value ten times too small. Only ever shrink the scale on the `Decimal` side and
    // do the multiplication in `U256`.
    let size = size.round_dp_with_strategy(decimals, RoundingStrategy::MidpointAwayFromZero);
    let mantissa = U256::from(size.mantissa().unsigned_abs());
    let exp = U256::from(decimals - size.scale());
    U256::from(10u8)
        .checked_pow(exp)
        .and_then(|factor| mantissa.checked_mul(factor))
        .expect("to_wei: amount does not fit in a U256")
}

/// Converts wei representation to a decimal amount.
///
/// # Parameters
///
/// - `wei`: The wei amount to convert
/// - `decimals`: Number of decimal places for the token (e.g., 18 for ETH, 6 for USDC)
///
/// # Example
///
/// Convert wei back to decimal: `from_wei(wei, 18)`
///
/// Digits beyond the 28 significant digits a `Decimal` holds are rounded.
///
/// # Panics
///
/// Panics if the amount exceeds [`Decimal::MAX`].
#[must_use]
#[inline]
pub fn from_wei(wei: U256, decimals: u32) -> Decimal {
    // `Decimal::from_i128_with_scale` panics once the wei value passes 2^96 (about
    // 1e11 tokens at 18 decimals), and `U256::to::<i128>` past `i128::MAX`. Split
    // the value at the decimal point instead and let `Decimal` parse it, which
    // keeps every representable amount and only rounds excess fractional digits.
    let digits = wei.to_string();
    let decimals = decimals as usize;
    let text = if digits.len() > decimals {
        let (int, frac) = digits.split_at(digits.len() - decimals);
        format!("{int}.{frac}")
    } else {
        format!("0.{digits:0>decimals$}")
    };
    Decimal::from_str(text.trim_end_matches('.')).unwrap_or_else(|_| {
        panic!("from_wei: {wei} wei at {decimals} decimals exceeds Decimal::MAX")
    })
}

#[cfg(test)]
mod tests {
    use rust_decimal::dec;

    use super::*;

    #[test]
    fn test_from_wei() {
        let test_values = [
            (
                U256::from(72305406316320073300i128),
                18,
                dec!(72.305406316320073300),
            ),
            (U256::from(98996405), 6, dec!(98.996405)),
            (U256::ZERO, 18, dec!(0)),
            (U256::from(5), 18, dec!(0.000000000000000005)),
            // Past 2^96 wei: 100 billion tokens at 18 decimals.
            (
                U256::from(10u8).pow(U256::from(29)),
                18,
                dec!(100_000_000_000),
            ),
            // Past i128::MAX wei.
            (
                U256::from(10u8).pow(U256::from(45)),
                18,
                dec!(1_000_000_000_000_000_000_000_000_000),
            ),
            // More decimals than a Decimal scale can hold.
            (U256::from(1), 30, dec!(0)),
            (U256::from(10u8).pow(U256::from(30)), 30, dec!(1)),
        ];
        for (index, (got, decimals, expect)) in test_values.into_iter().enumerate() {
            assert_eq!(from_wei(got, decimals), expect, "failed at {index}");
        }
    }

    #[test]
    fn test_to_wei() {
        let test_values = [
            (
                dec!(72.305406316320073386),
                18,
                U256::from(72305406316320073386i128),
            ),
            (dec!(98.996405), 6, U256::from(98996405)),
            (dec!(69), 6, U256::from(69000000)),
            (dec!(0), 18, U256::ZERO),
            (dec!(1.0000005), 6, U256::from(1000001)),
            // 100 billion tokens at 18 decimals needs a 97-bit mantissa.
            (
                dec!(100_000_000_000),
                18,
                U256::from(10u8).pow(U256::from(29)),
            ),
            (
                dec!(79228162514264337593543950335),
                18,
                U256::from(79228162514264337593543950335u128)
                    * U256::from(10u8).pow(U256::from(18)),
            ),
        ];
        for (index, (got, decimals, expect)) in test_values.into_iter().enumerate() {
            assert_eq!(to_wei(got, decimals), expect, "failed at {index}");
        }
    }

    #[test]
    #[should_panic(expected = "negative amount")]
    fn test_to_wei_rejects_negative() {
        let _ = to_wei(dec!(-1), 6);
    }

    #[test]
    #[should_panic(expected = "exceeds Decimal::MAX")]
    fn test_from_wei_panics_past_decimal_max() {
        let _ = from_wei(U256::MAX, 0);
    }
}
