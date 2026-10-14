//! Token unit conversions shared by HyperCore and HyperEVM.

use alloy::primitives::U256;
use rust_decimal::Decimal;

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
#[must_use]
#[inline]
pub fn to_wei(mut size: Decimal, decimals: u32) -> U256 {
    size.rescale(decimals);
    U256::from(size.mantissa())
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
#[must_use]
#[inline]
pub fn from_wei(wei: U256, decimals: u32) -> Decimal {
    Decimal::from_i128_with_scale(wei.to::<i128>(), decimals)
}
