//! HyperEVM interaction.
//!
//! Everything related to HyperEVM and contracts.

pub mod morpho;
pub mod uniswap;

// reimport
pub use alloy::providers::ProviderBuilder;
use alloy::{
    network::{Ethereum, IntoWallet},
    transports::TransportError,
};
/// reimport primitives
pub use alloy::{
    primitives::{Address, U256, address},
    providers::Provider as ProviderTrait,
    sol,
};
use rust_decimal::Decimal;

/// Default Hyperliquid RPC.
pub const DEFAULT_RPC_URL: &str = "https://rpc.hyperliquid.xyz/evm";
/// WHYPE contract address.
pub const WHYPE_ADDRESS: Address = address!("0x5555555555555555555555555555555555555555");

/// Custom provider trait rename
pub trait Provider: alloy::providers::Provider<Ethereum> + Send + Clone + 'static {}
/// Type alias for the dynamic provider.
pub type DynProvider = alloy::providers::DynProvider<Ethereum>;

impl<T> Provider for T where T: alloy::providers::Provider<Ethereum> + Send + Clone + 'static {}

sol! {
    #[sol(rpc)]
    interface IERC20 {
        // --- Metadata Functions ---
        function name() external view returns (string memory);
        function symbol() external view returns (string memory);
        function decimals() external view returns (uint8);

        // --- Core Functions (from IERC20) ---
        function totalSupply() external view returns (uint256);
        function balanceOf(address account) external view returns (uint256);
        function allowance(address owner, address spender) external view returns (uint256);
        function transfer(address to, uint256 amount) external returns (bool);
        function approve(address spender, uint256 amount) external returns (bool);
        function transferFrom(address from, address to, uint256 amount) external returns (bool);

        // --- Events (from IERC20) ---
        event Transfer(address indexed from, address indexed to, uint256 value);
        event Approval(address indexed owner, address indexed spender, uint256 value);
    }
}

/// Creates a Provider for Ethereum
#[inline(always)]
pub async fn mainnet() -> Result<impl Provider, TransportError> {
    mainnet_with_url(DEFAULT_RPC_URL).await
}

/// Creates a RootProvider
#[inline(always)]
pub async fn mainnet_with_signer<S>(signer: S) -> Result<impl Provider, TransportError>
where
    S: IntoWallet<Ethereum>,
    <S as IntoWallet<Ethereum>>::NetworkWallet: Clone + 'static,
{
    mainnet_with_signer_and_url(DEFAULT_RPC_URL, signer).await
}

/// Creates a RootProvider with a custom url
#[inline(always)]
pub async fn mainnet_with_url(url: &str) -> Result<impl Provider, TransportError> {
    let p = ProviderBuilder::new().connect(url).await?;
    Ok(p)
}

/// Creates a Provider with a custom url and signer
#[inline(always)]
pub async fn mainnet_with_signer_and_url<S>(
    url: &str,
    signer: S,
) -> Result<impl Provider, TransportError>
where
    S: IntoWallet<Ethereum>,
    <S as IntoWallet<Ethereum>>::NetworkWallet: Clone + 'static,
{
    let provider = ProviderBuilder::new().wallet(signer).connect(url).await?;
    Ok(provider)
}

/// Converts a number from Decimal to wei.
pub fn to_wei(mut size: Decimal, decimals: u32) -> U256 {
    size.rescale(decimals);
    U256::from(size.mantissa())
}

/// Converts a number from wei to Decimal.
pub fn from_wei(wei: U256, decimals: u32) -> Decimal {
    Decimal::from_i128_with_scale(wei.to::<i128>(), decimals)
}

#[cfg(test)]
mod tests {
    use alloy::{primitives::U256, providers::ProviderBuilder};
    use rust_decimal::dec;

    use super::*;
    use crate::hyperevm::DEFAULT_RPC_URL;

    const UBTC_ADDRESS: Address = address!("0x9fdbda0a5e284c32744d2f17ee5c74b284993463");

    #[tokio::test]
    async fn test_query() {
        let provider = ProviderBuilder::new().connect_http(DEFAULT_RPC_URL.parse().unwrap());
        let whype = IERC20::new(UBTC_ADDRESS, provider.clone());
        let balance = whype.totalSupply().call().await.unwrap();
        // let balance = utils::format_units(balance, 18).expect("ok");
        assert_eq!(balance, U256::from(21_000_000u128 * 100_000_000u128));
    }

    #[test]
    fn test_from_wei() {
        let test_values = [
            (
                U256::from(72305406316320073300i128),
                18,
                dec!(72.305406316320073300),
            ),
            (U256::from(98996405), 6, dec!(98.996405)),
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
        ];
        for (index, (got, decimals, expect)) in test_values.into_iter().enumerate() {
            assert_eq!(to_wei(got, decimals), expect, "failed at {index}");
        }
    }
}
