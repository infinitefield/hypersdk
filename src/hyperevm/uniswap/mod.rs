//! Uniswap V3 DEX integration.
//!
//! This module provides a client for interacting with Uniswap V3 on HyperEVM,
//! including querying pools, managing positions, and calculating prices.
//!
//! # Overview
//!
//! Uniswap V3 is a decentralized exchange protocol that enables:
//! - Automated market making with concentrated liquidity
//! - Price discovery for token pairs
//! - Liquidity provision with custom price ranges
//! - Flash swaps and complex trading strategies
//!
//! # Examples
//!
//! ## Query Pool Price
//!
//! ```no_run
//! use hypersdk::hyperevm::uniswap::{self, Contracts};
//! use hypersdk::Address;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let contracts = Contracts {
//!     factory: "0x...".parse()?,
//!     quoter: "0x...".parse()?,
//!     swap_router: "0x...".parse()?,
//!     non_fungible_position_manager: "0x...".parse()?,
//! };
//!
//! let client = uniswap::Client::mainnet(contracts).await?;
//!
//! let token0: Address = "0x...".parse()?;
//! let token1: Address = "0x...".parse()?;
//! let fee = 3000; // 0.3%
//!
//! let price = client.get_pool_price(token0, token1, fee).await?;
//! println!("Pool price: {}", price);
//! # Ok(())
//! # }
//! ```
//!
//! ## Query User Positions
//!
//! ```no_run
//! use hypersdk::hyperevm::uniswap::{self, Contracts};
//! use hypersdk::Address;
//!
//! # async fn example() -> anyhow::Result<()> {
//! # let contracts = Contracts {
//! #     factory: "0x...".parse()?,
//! #     quoter: "0x...".parse()?,
//! #     swap_router: "0x...".parse()?,
//! #     non_fungible_position_manager: "0x...".parse()?,
//! # };
//! let client = uniswap::Client::mainnet(contracts).await?;
//!
//! let user: Address = "0x...".parse()?;
//! let positions = client.positions(user).await?;
//!
//! for pos in positions {
//!     println!("Position {}: {} / {}",
//!         pos.token_id, pos.token0, pos.token1);
//!     println!("  Provided: {} / {}", pos.token0_provided, pos.token1_provided);
//!     println!("  Fees: {} / {}", pos.token0_fees, pos.token1_fees);
//! }
//! # Ok(())
//! # }
//! ```

pub mod contracts;
pub mod prjx;

use std::{
    collections::{HashMap, hash_map::Entry},
    hash::{DefaultHasher, Hash},
};

use alloy::{
    primitives::{U160, U256, aliases::U24},
    transports::TransportError,
};
use anyhow::Result;
use rust_decimal::{Decimal, MathematicalOps, dec, prelude::ToPrimitive};

use crate::hyperevm::{
    Address, DynProvider, ERC20, Provider,
    uniswap::contracts::{
        INonfungiblePositionManager::{self, CollectParams, INonfungiblePositionManagerInstance},
        IQuoterV2::{self, IQuoterV2Instance},
        ISwapRouter::{self, ISwapRouterInstance},
        IUniswapV3Factory::{self, IUniswapV3FactoryInstance},
        IUniswapV3Pool::{self, IUniswapV3PoolInstance},
    },
};

/// Standard Uniswap V3 fee tiers (in basis points).
///
/// Uniswap V3 supports multiple fee tiers for different volatility levels:
/// - `100`: 0.01% (for stable pairs like USDC/USDT)
/// - `500`: 0.05% (for less volatile pairs)
/// - `3_000`: 0.3% (most common, for standard pairs)
/// - `10_000`: 1% (for exotic or highly volatile pairs)
pub const FEES: [u32; 4] = [
    100,    // 0.01%
    500,    // 0.05%
    3_000,  // 0.3%
    10_000, // 1%
];

#[inline(always)]
fn tick_to_sqrt_price(tick: i64) -> Decimal {
    let price = dec!(1.0001).powi(tick);
    price.sqrt().unwrap()
}

fn tick_in_range<T: Ord>(tick: T, lower: T, upper: T) -> bool {
    (lower..upper).contains(&tick)
}

#[cfg(test)]
mod range_tests {
    use super::tick_in_range;

    #[test]
    fn position_range_is_half_open() {
        for (tick, expected) in [(-11, false), (-10, true), (0, true), (10, false), (11, false)] {
            assert_eq!(tick_in_range(tick, -10, 10), expected, "tick {tick}");
        }
    }
}

// https://github.com/Uniswap/v3-core/blob/d8b1c635c275d2a9450bd6a78f3fa2484fef73eb/contracts/libraries/TickMath.sol
fn get_amounts_from_liquidity(
    liquidity: u128,
    tick_lower: i64,
    tick_upper: i64,
    tick_current: i64,
) -> (Decimal, Decimal) {
    let liquidity_f64 = Decimal::from(liquidity);

    let sqrt_lower = tick_to_sqrt_price(tick_lower);
    let sqrt_upper = tick_to_sqrt_price(tick_upper);
    let sqrt_price = tick_to_sqrt_price(tick_current);

    if tick_current <= tick_lower {
        let amount0 = liquidity_f64 * (sqrt_upper - sqrt_lower) / (sqrt_upper * sqrt_lower);
        return (amount0, Decimal::ZERO);
    }

    if tick_current >= tick_upper {
        let amount1 = liquidity_f64 * (sqrt_upper - sqrt_lower);
        return (Decimal::ZERO, amount1);
    }

    let amount0 = liquidity_f64 * (sqrt_upper - sqrt_price) / (sqrt_upper * sqrt_price);
    let amount1 = liquidity_f64 * (sqrt_price - sqrt_lower);

    (amount0, amount1)
}

/// Converts a price to Uniswap's sqrtPriceLimitX96 format.
///
/// This is an approximation since [`Decimal`] can't store the full Q64.96 precision.
///
/// # Parameters
///
/// - `price`: The price to convert
/// - `scale`: Decimal scaling factor
///
/// # Example
///
/// Convert a price to sqrtPriceLimitX96 format: `sqrt_price_limit_x96(price, scale)`
#[must_use]
pub fn sqrt_price_limit_x96(price: Decimal, scale: u32) -> U160 {
    let q96 = U160::from(2).pow(U160::from(96));
    let price = U160::from((price * Decimal::TEN.powi(scale as i64)).to_i128().unwrap());
    let sqrt = price.root(2);
    // sqrt * q96 / 18 digits (evm default max digits)
    sqrt * q96 / U160::from(10).pow(U160::from(18))
}

/// Converts Uniswap's sqrtPriceX96 format to a decimal price.
///
/// This is an approximation since [`Decimal`] can't store the full Q64.96 precision.
///
/// # Parameters
///
/// - `sqrt_price_x96`: The sqrt price in Q64.96 format
/// - `decimals0`: Decimals of token0
/// - `decimals1`: Decimals of token1
///
/// # Returns
///
/// The price of token0 in terms of token1.
///
/// # Example
///
/// ```
/// use hypersdk::hyperevm::uniswap::sqrt_x96_to_price;
/// use hypersdk::U160;
///
/// # let sqrt_price_x96 = U160::from(1u64);
/// let price = sqrt_x96_to_price(sqrt_price_x96, 18, 6);
/// println!("Price: {}", price);
/// ```
#[must_use]
pub fn sqrt_x96_to_price(sqrt_price_x96: U160, decimals0: u32, decimals1: u32) -> Decimal {
    let q96 = U160::from(2).pow(U160::from(96));

    // Scale sqrt_price to avoid precision loss
    let sqrt_price_scaled = sqrt_price_x96 * U160::from(10).pow(U160::from(decimals0));

    let price = (sqrt_price_scaled / q96).pow(U160::from(2));
    Decimal::from_i128_with_scale(price.to::<i128>(), decimals0 + decimals1)
}

/// Uniswap V3 contract addresses.
///
/// Contains the addresses of the core Uniswap V3 contracts needed for interaction.
///
/// # Example
///
/// ```
/// use hypersdk::hyperevm::uniswap::Contracts;
/// use hypersdk::Address;
///
/// # fn example() -> anyhow::Result<()> {
/// let contracts = Contracts {
///     factory: "0x...".parse()?,
///     quoter: "0x...".parse()?,
///     swap_router: "0x...".parse()?,
///     non_fungible_position_manager: "0x...".parse()?,
/// };
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Contracts {
    /// UniswapV3Factory contract address
    pub factory: Address,
    /// QuoterV2 contract address (for price quotes)
    pub quoter: Address,
    /// SwapRouter contract address (for executing swaps)
    pub swap_router: Address,
    /// NonfungiblePositionManager contract address (for LP positions)
    pub non_fungible_position_manager: Address,
}

/// A Uniswap V3 liquidity position.
///
/// Represents an NFT liquidity position with token amounts and accumulated fees.
///
/// # Example
///
/// Query user positions: `client.positions(user_address).await?`
/// Check if position is active with `pos.in_range` field.
#[derive(Debug, Clone)]
pub struct Position {
    /// NFT token ID for this position
    pub token_id: U256,
    /// First token in the pair
    pub token0: Address,
    /// Second token in the pair
    pub token1: Address,
    /// Amount of token0 provided as liquidity
    pub token0_provided: Decimal,
    /// Amount of token1 provided as liquidity
    pub token1_provided: Decimal,
    /// Accumulated fees in token0
    pub token0_fees: Decimal,
    /// Accumulated fees in token1
    pub token1_fees: Decimal,
    /// Whether the position is in range (actively earning fees)
    pub in_range: bool,
}

impl Position {
    /// Returns whether the position is actively earning fees.
    ///
    /// A position is "in range" when the current pool price is within
    /// the position's price range.
    #[must_use]
    #[inline]
    pub fn is_active(&self) -> bool {
        self.in_range
    }

    /// Returns the total value provided to the position in token0 equivalent.
    ///
    /// This converts token1 to token0 using the given price.
    ///
    /// # Parameters
    ///
    /// - `token1_price_in_token0`: The price of token1 in terms of token0
    #[must_use]
    pub fn total_value_in_token0(&self, token1_price_in_token0: Decimal) -> Decimal {
        self.token0_provided + (self.token1_provided * token1_price_in_token0)
    }

    /// Returns the total fees earned in token0 equivalent.
    ///
    /// # Parameters
    ///
    /// - `token1_price_in_token0`: The price of token1 in terms of token0
    #[must_use]
    pub fn total_fees_in_token0(&self, token1_price_in_token0: Decimal) -> Decimal {
        self.token0_fees + (self.token1_fees * token1_price_in_token0)
    }
}

/// Client for Uniswap V3 DEX.
///
/// Provides methods for querying pools, positions, and executing swaps.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hyperevm::uniswap::{self, Contracts};
///
/// # async fn example() -> anyhow::Result<()> {
/// let contracts = Contracts {
///     factory: "0x...".parse()?,
///     quoter: "0x...".parse()?,
///     swap_router: "0x...".parse()?,
///     non_fungible_position_manager: "0x...".parse()?,
/// };
///
/// let client = uniswap::Client::mainnet(contracts).await?;
/// # Ok(())
/// # }
/// ```
pub struct Client<P>
where
    P: Provider,
{
    provider: P,
    contracts: Contracts,
}

impl Client<DynProvider> {
    /// Creates a client for mainnet.
    pub async fn mainnet(contracts: Contracts) -> Result<Self, TransportError> {
        let provider = DynProvider::new(super::mainnet().await?);
        Ok(Self::new(provider, contracts))
    }

    /// Creates a client for mainnet.
    pub async fn mainnet_with_url(url: &str, contracts: Contracts) -> Result<Self, TransportError> {
        let provider = DynProvider::new(super::mainnet_with_url(url).await?);
        Ok(Self::new(provider, contracts))
    }
}

impl<P> Client<P>
where
    P: Provider,
{
    /// Create a uniswap client.
    pub fn new(provider: P, contracts: Contracts) -> Self {
        Self {
            provider,
            contracts,
        }
    }

    /// Returns the root provider.
    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Returns the uniswap factory.
    pub fn factory(&self) -> IUniswapV3FactoryInstance<P> {
        IUniswapV3Factory::new(self.contracts.factory, self.provider().clone())
    }

    /// Returns the uniswap pool.
    pub fn pool(&self, address: Address) -> IUniswapV3PoolInstance<P> {
        IUniswapV3Pool::new(address, self.provider().clone())
    }

    /// Returns the uniswap quoter.
    pub fn quoter(&self) -> IQuoterV2Instance<P> {
        IQuoterV2::new(self.contracts.quoter, self.provider().clone())
    }

    /// Returns the uniswap swap router.
    pub fn swap_router(&self) -> ISwapRouterInstance<P> {
        ISwapRouter::new(self.contracts.swap_router, self.provider().clone())
    }

    /// Returns the uniswap non-fungible positions manager.
    pub fn non_fungible_position_manager(&self) -> INonfungiblePositionManagerInstance<P> {
        INonfungiblePositionManager::new(
            self.contracts.non_fungible_position_manager,
            self.provider().clone(),
        )
    }

    /// Load the current positions from a user.
    ///
    /// TODO: make it composable so a user could query a specific block, ...
    pub async fn positions(&self, target_address: Address) -> Result<Vec<Position>> {
        let npm = self.non_fungible_position_manager();
        let factory = self.factory();

        let position_count: U256 = npm.balanceOf(target_address).call().await?;
        let count = position_count.to::<usize>();

        let mut positions = vec![];

        struct PositionData {
            decimals0: u8,
            decimals1: u8,
            pool_address: Address,
        }

        let mut pools: HashMap<u64, PositionData> = HashMap::default();

        for i in 0..count {
            let token_id: U256 = npm
                .tokenOfOwnerByIndex(target_address, U256::from(i))
                .call()
                .await?;

            let pos = npm.positions(token_id).call().await?;
            if pos.liquidity == 0 {
                continue;
            }

            use std::hash::Hasher;
            let mut hasher = DefaultHasher::default();
            pos.token0.hash(&mut hasher);
            pos.token1.hash(&mut hasher);
            pos.fee.hash(&mut hasher);

            let prehash = hasher.finish();
            let entry = pools.entry(prehash);
            if let Entry::Vacant(entry) = entry {
                let token0_client = ERC20::new(pos.token0, self.provider.clone());
                let token1_client = ERC20::new(pos.token1, self.provider.clone());

                let (decimals0, decimals1, pool_address) = self
                    .provider
                    .multicall()
                    .add(token0_client.decimals())
                    .add(token1_client.decimals())
                    .add(factory.getPool(pos.token0, pos.token1, pos.fee))
                    .aggregate()
                    .await?;
                entry.insert(PositionData {
                    decimals0,
                    decimals1,
                    pool_address,
                });

                if pool_address.is_zero() {
                    continue;
                }
            }

            let pools = &pools[&prehash];
            let (decimals0, decimals1, pool_address) =
                (pools.decimals0, pools.decimals1, pools.pool_address);

            let max_u128: u128 = u128::MAX;
            let params = CollectParams {
                tokenId: token_id,
                recipient: target_address,
                amount0Max: max_u128,
                amount1Max: max_u128,
            };

            let collect_call = npm.collect(params);
            let res = collect_call.from(target_address).call().await?;

            use std::convert::TryFrom;
            let fees_in_0 = Decimal::from(u128::try_from(res.amount0)?);
            let fees_in_1 = Decimal::from(u128::try_from(res.amount1)?);

            let token0_fees = fees_in_0 / Decimal::TEN.powi(decimals0 as i64);
            let token1_fees = fees_in_1 / Decimal::TEN.powi(decimals1 as i64);

            let pool = self.pool(pool_address);
            let slot0 = pool.slot0().call().await?;

            let in_range = tick_in_range(slot0.tick, pos.tickLower, pos.tickUpper);

            let (amount0_raw, amount1_raw) = get_amounts_from_liquidity(
                pos.liquidity as u128,
                pos.tickLower.try_into()?,
                pos.tickUpper.try_into()?,
                slot0.tick.try_into()?,
            );

            let amount0_in_token = amount0_raw / Decimal::TEN.powi(decimals0 as i64);
            let amount1_in_token = amount1_raw / Decimal::TEN.powi(decimals1 as i64);
            positions.push(Position {
                token_id,
                token0: pos.token0,
                token1: pos.token1,
                token0_provided: amount0_in_token,
                token1_provided: amount1_in_token,
                token0_fees,
                token1_fees,
                in_range,
            });
        }

        Ok(positions)
    }

    /// Resolves the Uniswap V3 pool contract for `token0`, `token1`, and `fee`.
    pub async fn get_pool_address(
        &self,
        token0: Address,
        token1: Address,
        fee: u32,
    ) -> Result<Address> {
        let factory = self.factory();
        let token0_erc = ERC20::new(token0, self.provider.clone());
        let token1_erc = ERC20::new(token1, self.provider.clone());
        let (_, _, address) = self
            .provider
            .multicall()
            .add(token0_erc.symbol())
            .add(token1_erc.symbol())
            .add(factory.getPool(token0, token1, U24::from(fee)))
            .aggregate()
            .await?;
        Ok(address)
    }

    /// Get the price from a pool.
    pub async fn pool_price_sqrt_x96(
        &self,
        token0: Address,
        token1: Address,
        fee: u32,
    ) -> Result<U160> {
        let factory = self.factory();
        let pool_address = factory
            .getPool(token0, token1, U24::from(fee))
            .call()
            .await?;
        let pool = self.pool(pool_address);
        let slot0 = pool.slot0().call().await?;
        Ok(slot0.sqrtPriceX96)
    }

    /// Returns the pool's slot0.
    pub async fn slot0(
        &self,
        token0: Address,
        token1: Address,
        fee: u32,
    ) -> Result<IUniswapV3Pool::slot0Return> {
        let factory = self.factory();
        let pool_address = factory
            .getPool(token0, token1, U24::from(fee))
            .call()
            .await?;
        let pool = self.pool(pool_address);
        let ret = pool.slot0().call().await?;
        Ok(ret)
    }

    /// Get the pool's price in a Decimal approximation.
    pub async fn get_pool_price(
        &self,
        token0: Address,
        token1: Address,
        fee: u32,
    ) -> Result<Decimal> {
        let factory = self.factory();

        let token0_client = ERC20::new(token0, self.provider.clone());
        let token1_client = ERC20::new(token1, self.provider.clone());

        // get the pool address and the decimals of each token
        let (decimals0, decimals1, pool_address) = self
            .provider
            .multicall()
            .add(token0_client.decimals())
            .add(token1_client.decimals())
            .add(factory.getPool(token0, token1, U24::from(fee)))
            .aggregate()
            .await?;

        let pool = self.pool(pool_address);
        let slot0 = pool.slot0().call().await?;

        Ok(sqrt_x96_to_price(
            slot0.sqrtPriceX96,
            decimals0 as u32,
            decimals1 as u32,
        ))
    }

    /// Get the pool's price in a Decimal approximation.
    pub async fn pool_price_from(&self, pool_address: Address) -> Result<Decimal> {
        let pool = self.pool(pool_address);

        let (token0, token1) = self
            .provider
            .multicall()
            .add(pool.token0())
            .add(pool.token1())
            .aggregate()
            .await?;

        let token0_client = ERC20::new(token0, self.provider.clone());
        let token1_client = ERC20::new(token1, self.provider.clone());

        let (decimals0, decimals1) = self
            .provider
            .multicall()
            .add(token0_client.decimals())
            .add(token1_client.decimals())
            .aggregate()
            .await?;

        let pool = self.pool(pool_address);
        let slot0 = pool.slot0().call().await?;

        Ok(sqrt_x96_to_price(
            slot0.sqrtPriceX96,
            decimals0 as u32,
            decimals1 as u32,
        ))
    }
}
