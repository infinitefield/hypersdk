//! Uniswap contract calls

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
        CollectParams,
        Factory::{self, FactoryInstance},
        NFTPositionManager,
        Pool::{self, PoolInstance, slot0Return},
        QuoterV2::{self, QuoterV2Instance},
        SwapRouter::{self, SwapRouterInstance},
    },
};

/// Uniswap fees.
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

/// Convert a price to sqrtPriceLimitX96.
///
/// This approach is an approximation since [`Decimal`] can't store the maximum precision.
pub fn sqrt_price_limit_x96(price: Decimal, scale: u32) -> U160 {
    let q96 = U160::from(2).pow(U160::from(96));
    let price = U160::from((price * Decimal::TEN.powi(scale as i64)).to_i128().unwrap());
    let sqrt = price.root(2);
    // sqrt * q96 / 18 digits (evm default max digits)
    sqrt * q96 / U160::from(10).pow(U160::from(18))
}

/// Convert an uniswap price to Decimal.
///
/// This approach is an approximation since [`Decimal`] can't store the maximum precision.
pub fn sqrt_x96_to_price(sqrt_price_x96: U160, decimals0: u32, decimals1: u32) -> Decimal {
    let q96 = U160::from(2).pow(U160::from(96));

    // because sqrt_price could be less than q96, we need to scale by `scale`.
    let sqrt_price_scaled = sqrt_price_x96 * U160::from(10).pow(U160::from(decimals0));

    let price = (sqrt_price_scaled / q96).pow(U160::from(2));
    Decimal::from_i128_with_scale(price.to::<i128>(), decimals0 + decimals1)
}

/// Uniswap contract addresses.
#[derive(Debug, Clone, Copy)]
pub struct Contracts {
    pub factory: Address,
    pub quoter: Address,
    pub swap_router: Address,
    pub non_fungible_position_manager: Address,
    // pub non_fungible_position_description: Address,
}

/// Uniswap position
#[derive(Debug, Clone)]
pub struct Position {
    pub token_id: U256,
    pub token0: Address,
    pub token1: Address,
    pub token0_provided: Decimal,
    pub token1_provided: Decimal,
    pub token0_fees: Decimal,
    pub token1_fees: Decimal,
    pub in_range: bool,
}

/// Uniswap client
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
    pub fn factory(&self) -> FactoryInstance<P> {
        Factory::new(self.contracts.factory, self.provider().clone())
    }

    /// Returns the uniswap pool.
    pub fn pool(&self, address: Address) -> PoolInstance<P> {
        Pool::new(address, self.provider().clone())
    }

    /// Returns the uniswap quoter.
    pub fn quoter(&self) -> QuoterV2Instance<P> {
        QuoterV2::new(self.contracts.quoter, self.provider().clone())
    }

    /// Returns the uniswap swap router.
    pub fn swap_router(&self) -> SwapRouterInstance<P> {
        SwapRouter::new(self.contracts.swap_router, self.provider().clone())
    }

    /// Load the current positions from a user.
    ///
    /// TODO: make it composable so a user could query a specific block, ...
    pub async fn positions(&self, target_address: Address) -> Result<Vec<Position>> {
        let npm = NFTPositionManager::new(
            self.contracts.non_fungible_position_manager,
            self.provider.clone(),
        );
        let factory = Factory::new(self.contracts.factory, self.provider.clone());

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

            let pool = Pool::new(pool_address, self.provider.clone());
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

    /// Get the pool address.
    pub async fn get_pool_addres(
        &self,
        token0: Address,
        token1: Address,
        fee: u32,
    ) -> Result<Address> {
        let factory = Factory::new(self.contracts.factory, self.provider.clone());
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
        let pool = Pool::new(pool_address, self.provider.clone());
        let slot0 = pool.slot0().call().await?;
        Ok(slot0.sqrtPriceX96)
    }

    /// Returns the pool's slot0.
    pub async fn slot0(&self, token0: Address, token1: Address, fee: u32) -> Result<slot0Return> {
        let factory = self.factory();
        let pool_address = factory
            .getPool(token0, token1, U24::from(fee))
            .call()
            .await?;
        let pool = Pool::new(pool_address, self.provider.clone());
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

        let pool = Pool::new(pool_address, self.provider.clone());
        let slot0 = pool.slot0().call().await?;

        Ok(sqrt_x96_to_price(
            slot0.sqrtPriceX96,
            decimals0 as u32,
            decimals1 as u32,
        ))
    }

    /// Get the pool's price in a Decimal approximation.
    pub async fn pool_price_from(&self, pool_address: Address) -> Result<Decimal> {
        let pool = Pool::new(pool_address, self.provider().clone());

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

        let pool = Pool::new(pool_address, self.provider.clone());
        let slot0 = pool.slot0().call().await?;

        Ok(sqrt_x96_to_price(
            slot0.sqrtPriceX96,
            decimals0 as u32,
            decimals1 as u32,
        ))
    }

    // /// Close a position.
    // pub async fn close_position(&self, token_id: U256, owner: Address) -> Result<()> {
    //     let npm = NFTPositionManager::new(
    //         self.contracts.non_fungible_position_manager,
    //         self.provider.clone(),
    //     );

    //     let position_data = npm.positions(token_id).call().await?;
    //     if position_data.liquidity == 0 {
    //         return Ok(());
    //     }

    //     let now_secs = SystemTime::now()
    //         .duration_since(UNIX_EPOCH)
    //         .unwrap()
    //         .as_secs();
    //     let deadline = U256::from(now_secs + 600);

    //     let params = DecreaseLiquidityParams {
    //         tokenId: token_id,
    //         liquidity: position_data.liquidity,
    //         amount0Min: U256::from(0),
    //         amount1Min: U256::from(0),
    //         deadline,
    //     };

    //     let _res = npm.decreaseLiquidity(params).from(owner).send().await?;
    //     let max_u128: u128 = u128::MAX;
    //     let collect_params = CollectParams {
    //         tokenId: token_id,
    //         recipient: owner,
    //         amount0Max: max_u128,
    //         amount1Max: max_u128,
    //     };
    //     let _res_collect = npm.collect(collect_params).from(owner).send().await?;

    //     //let _res = self.npm.burn(token_id).from(owner).send().await?;

    //     Ok(())
    // }
}
