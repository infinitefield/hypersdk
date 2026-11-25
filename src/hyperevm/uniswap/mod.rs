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
    primitives::{U160, U256, U512, aliases::U24},
    transports::TransportError,
};
use anyhow::{Context, Result};
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
/// Computes `(sqrt_price_x96 / 2^96)^2 * 10^(decimals0 - decimals1)` and rounds
/// only at the final conversion to [`Decimal`].
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
/// # Panics
///
/// Panics if the resulting price exceeds [`Decimal::MAX`].
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
    price_from_sqrt_x96(sqrt_price_x96, decimals0, decimals1).unwrap_or_else(|| {
        panic!(
            "sqrt_x96_to_price: {sqrt_price_x96} with decimals ({decimals0}, {decimals1}) exceeds Decimal::MAX"
        )
    })
}

fn price_from_sqrt_x96(sqrt_price_x96: U160, decimals0: u32, decimals1: u32) -> Option<Decimal> {
    if sqrt_price_x96.is_zero() {
        return Some(Decimal::ZERO);
    }

    let exp10 = i64::from(decimals0) - i64::from(decimals1);
    // For 1 <= sqrt_price_x96 < 2^160, (sqrt_price_x96 / 2^96)^2 lies in [2^-192, 2^128):
    // below 10^-67 the product is under 0.5 * 10^-28 and rounds to zero; above 10^86 it
    // exceeds Decimal::MAX (~7.92e28).
    if exp10 < -67 {
        return Some(Decimal::ZERO);
    }
    if exp10 > 86 {
        return None;
    }

    let s = U512::from(sqrt_price_x96);
    let s_sq = s * s;

    for scale in (0..=Decimal::MAX_SCALE).rev() {
        if let Some(price) = round_sqrt_x96_sq_at_scale(s_sq, exp10 + i64::from(scale), scale) {
            return Some(price);
        }
    }
    None
}

fn round_sqrt_x96_sq_at_scale(s_sq: U512, exp: i64, scale: u32) -> Option<Decimal> {
    let rounded = if exp >= 0 {
        let pow10 = U512::from(10u8).checked_pow(U512::from(exp as u64))?;
        let num = s_sq.checked_mul(pow10)?;
        let q: U512 = num >> 192;
        let rem = num & ((U512::ONE << 192) - U512::ONE);
        let half = U512::ONE << 191;
        if rem > half || (rem == half && q.bit(0)) {
            q + U512::ONE
        } else {
            q
        }
    } else {
        let pow10 = U512::from(10u8).checked_pow(U512::from((-exp) as u64))?;
        let den = pow10.checked_shl(192)?;
        let (q, rem) = s_sq.div_rem(den);
        let half: U512 = den >> 1;
        if rem > half || (rem == half && q.bit(0)) {
            q + U512::ONE
        } else {
            q
        }
    };

    let mantissa = i128::try_from(rounded).ok()?;
    Decimal::try_from_i128_with_scale(mantissa, scale)
        .ok()
        .map(|d| d.normalize())
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

            let in_range = slot0.tick <= pos.tickUpper && slot0.tick >= pos.tickLower;

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

        price_from_sqrt_x96(
            slot0.sqrtPriceX96,
            u32::from(decimals0),
            u32::from(decimals1),
        )
        .context("pool price exceeds Decimal::MAX")
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

        price_from_sqrt_x96(
            slot0.sqrtPriceX96,
            u32::from(decimals0),
            u32::from(decimals1),
        )
        .context("pool price exceeds Decimal::MAX")
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::U160;
    use rust_decimal::{Decimal, dec};

    use super::{price_from_sqrt_x96, sqrt_x96_to_price};

    #[test]
    fn parity_with_equal_18_decimals_returns_one() {
        let price = sqrt_x96_to_price(U160::from(1u8) << 96, 18, 18);
        assert_eq!(price, Decimal::ONE);
        assert_eq!(price.scale(), 0);
    }

    #[test]
    fn fractional_prices_are_not_truncated_before_squaring() {
        assert_eq!(sqrt_x96_to_price(U160::from(1u8) << 95, 0, 0), dec!(0.25));
        assert_eq!(sqrt_x96_to_price(U160::from(3u8) << 95, 0, 0), dec!(2.25));
        // (2^-10)^2 = 2^-20 = 0.00000095367431640625 (exact in 20 decimal places).
        assert_eq!(
            sqrt_x96_to_price(U160::from(1u8) << 86, 0, 0),
            dec!(0.00000095367431640625)
        );
        // ((2^96 - 1) / 3 / 2^96)^2 = 1/9 to 28 decimal places.
        assert_eq!(
            sqrt_x96_to_price((U160::from(1u8) << 96) / U160::from(3u8), 18, 18),
            dec!(0.1111111111111111111111111111)
        );
    }

    #[test]
    fn handles_decimal_offsets_zero_and_large_ratios() {
        assert_eq!(sqrt_x96_to_price(U160::ZERO, 18, 18), Decimal::ZERO);
        assert_eq!(sqrt_x96_to_price(U160::ZERO, u32::MAX, 0), Decimal::ZERO);
        assert_eq!(
            sqrt_x96_to_price(U160::from(1u8) << 96, u32::MAX, u32::MAX),
            Decimal::ONE
        );
        // 50^2 = 2500 on an 18/18 pair; previously overflowed U160 when scaling by 10^18.
        assert_eq!(
            sqrt_x96_to_price(U160::from(50u8) << 96, 18, 18),
            dec!(2500)
        );
        assert_eq!(
            sqrt_x96_to_price(U160::from(1u8) << 96, 18, 6),
            dec!(1_000_000_000_000)
        );
        assert_eq!(
            sqrt_x96_to_price(U160::from(1u8) << 96, 6, 18),
            dec!(0.000000000001)
        );
        assert_eq!(
            sqrt_x96_to_price(U160::from(20_000u32) << 96, 6, 18),
            dec!(0.0004)
        );
        // Values below 0.5 * 10^-28 round to zero instead of panicking.
        assert_eq!(sqrt_x96_to_price(U160::from(1u8), 18, 6), Decimal::ZERO);
        assert_eq!(sqrt_x96_to_price(U160::MAX, 0, u32::MAX), Decimal::ZERO);
    }

    #[test]
    fn decimal_max_boundary() {
        // ((2^144 - 2^47) / 2^96)^2 = 2^96 - 1 + 2^-98, which rounds to Decimal::MAX.
        let at_max = (U160::from(1u8) << 144) - (U160::from(1u8) << 47);
        assert_eq!(sqrt_x96_to_price(at_max, 0, 0), Decimal::MAX);

        // ((2^144 - 2^46) / 2^96)^2 = 2^96 - 0.5 + 2^-100, which rounds to 2^96 > Decimal::MAX.
        let past_max = (U160::from(1u8) << 144) - (U160::from(1u8) << 46);
        assert_eq!(price_from_sqrt_x96(past_max, 0, 0), None);
        assert_eq!(price_from_sqrt_x96(U160::MAX, 0, 0), None);
        assert_eq!(price_from_sqrt_x96(U160::from(1u8) << 96, 29, 0), None);
    }

    #[test]
    #[should_panic(expected = "exceeds Decimal::MAX")]
    fn panics_past_decimal_max() {
        let _ = sqrt_x96_to_price(U160::MAX, 0, 0);
    }
}
