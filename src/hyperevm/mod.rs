//! HyperEVM Ethereum-compatible layer.
//!
//! This module provides functionality for interacting with HyperEVM, Hyperliquid's
//! Ethereum-compatible layer. You can interact with any EVM contract, with specialized
//! support for DeFi protocols like Morpho and Uniswap.
//!
//! # Overview
//!
//! HyperEVM is built on the Alloy Ethereum library, providing:
//! - ERC-20 token interactions
//! - Smart contract calls
//! - Transaction signing and submission
//! - Event filtering and logs
//!
//! # Submodules
//!
//! - [`morpho`]: Morpho Blue lending protocol integration
//! - [`uniswap`]: Uniswap V3 DEX integration
//!
//! # Examples
//!
//! ## Create a Provider
//!
//! Create a mainnet provider with `hyperevm::mainnet().await?` and query block numbers.
//!
//! ## Interact with ERC-20 Tokens
//!
//! ```no_run
//! use hypersdk::hyperevm::{self, ERC20, Address};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let provider = hyperevm::mainnet().await?;
//! let token_address: Address = "0x...".parse()?;
//! let token = ERC20::new(token_address, provider);
//!
//! // Query token info
//! let symbol = token.symbol().call().await?;
//! let decimals = token.decimals().call().await?;
//! let total_supply = token.totalSupply().call().await?;
//!
//! println!("{} ({} decimals): {} total supply", symbol, decimals, total_supply);
//! # Ok(())
//! # }
//! ```
//!
//! ## Wei Conversions
//!
//! Convert between decimal amounts and wei using `to_wei(amount, decimals)` and `from_wei(wei, decimals)`.

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

/// Default HyperEVM RPC URL.
///
/// URL: `https://rpc.hyperliquid.xyz/evm`
pub const DEFAULT_RPC_URL: &str = "https://rpc.hyperliquid.xyz/evm";

/// WHYPE (Wrapped HYPE) contract address on HyperEVM.
pub const WHYPE_ADDRESS: Address = address!("0x5555555555555555555555555555555555555555");

/// Provider trait for HyperEVM.
///
/// This trait is implemented by all Alloy providers and ensures they can be
/// used with HyperEVM contract interactions.
pub trait Provider: alloy::providers::Provider<Ethereum> + Send + Clone + 'static {}

/// Dynamic provider type for HyperEVM.
///
/// Use this when you need type erasure for providers.
pub type DynProvider = alloy::providers::DynProvider<Ethereum>;

impl<T> Provider for T where T: alloy::providers::Provider<Ethereum> + Send + Clone + 'static {}

sol!(
    #[sol(rpc)]
    ERC20,
    "abi/ERC20.json"
);

sol!(
    #[sol(rpc)]
    IERC4626,
    "abi/IERC4626.json"
);

sol!(
    #[sol(rpc)]
    IERC777,
    "abi/IERC777.json"
);

/// Creates a provider for HyperEVM mainnet.
///
/// Connects to the default HyperEVM RPC endpoint.
///
/// # Example
///
/// Create a mainnet provider: `hyperevm::mainnet().await?`
#[inline(always)]
pub async fn mainnet() -> Result<impl Provider, TransportError> {
    mainnet_with_url(DEFAULT_RPC_URL).await
}

/// Creates a provider with a signer for HyperEVM mainnet.
///
/// This allows you to send transactions that modify blockchain state.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hyperevm;
/// use alloy::signers::local::PrivateKeySigner;
///
/// # async fn example() -> anyhow::Result<()> {
/// let signer: PrivateKeySigner = "your_key".parse()?;
/// let provider = hyperevm::mainnet_with_signer(signer).await?;
/// // Can now send transactions
/// # Ok(())
/// # }
/// ```
#[inline(always)]
pub async fn mainnet_with_signer<S>(signer: S) -> Result<impl Provider, TransportError>
where
    S: IntoWallet<Ethereum>,
    <S as IntoWallet<Ethereum>>::NetworkWallet: Clone + 'static,
{
    mainnet_with_signer_and_url(DEFAULT_RPC_URL, signer).await
}

/// Creates a provider with a custom RPC URL.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hyperevm;
///
/// # async fn example() -> anyhow::Result<()> {
/// let provider = hyperevm::mainnet_with_url("https://custom-rpc.example.com").await?;
/// # Ok(())
/// # }
/// ```
#[inline(always)]
pub async fn mainnet_with_url(url: &str) -> Result<impl Provider, TransportError> {
    let p = ProviderBuilder::new().connect(url).await?;
    Ok(p)
}

/// Creates a provider with a custom RPC URL and signer.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hyperevm;
/// use alloy::signers::local::PrivateKeySigner;
///
/// # async fn example() -> anyhow::Result<()> {
/// let signer: PrivateKeySigner = "your_key".parse()?;
/// let provider = hypersdk::hyperevm::mainnet_with_signer_and_url(
///     "https://custom-rpc.example.com",
///     signer
/// ).await?;
/// # Ok(())
/// # }
/// ```
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

pub use crate::units::{from_wei, to_wei};

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
        let whype = ERC20::new(UBTC_ADDRESS, provider.clone());
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
