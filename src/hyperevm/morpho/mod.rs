//! Morpho Blue lending protocol integration.
//!
//! This module provides clients for interacting with Morpho Blue, a decentralized
//! lending protocol, and MetaMorpho vaults on HyperEVM.
//!
//! # Overview
//!
//! Morpho Blue is an efficient lending protocol that allows users to:
//! - Supply assets to earn interest
//! - Borrow assets with collateral
//! - Create isolated lending markets
//!
//! MetaMorpho vaults aggregate multiple Morpho markets to optimize yields.
//!
//! # Clients
//!
//! - [`Client`]: For interacting with individual Morpho Blue markets
//! - [`MetaClient`]: For interacting with MetaMorpho vaults
//!
//! # Examples
//!
//! ## Query Market APY
//!
//! ```no_run
//! use hypersdk::hyperevm::morpho;
//! use hypersdk::Address;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let client = morpho::Client::mainnet().await?;
//!
//! let morpho_addr: Address = "0x...".parse()?;
//! let market_id = [0u8; 32].into();
//!
//! let apy = client.apy::<f64, _>(morpho_addr, market_id, |e| e.exp()).await?;
//! println!("Borrow APY: {:.2}%", apy.borrow * 100.0);
//! println!("Supply APY: {:.2}%", apy.supply * 100.0);
//! # Ok(())
//! # }
//! ```
//!
//! ## Query MetaMorpho Vault APY
//!
//! ```no_run
//! use hypersdk::hyperevm::morpho;
//! use hypersdk::Address;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let client = morpho::MetaClient::mainnet().await?;
//!
//! let vault_addr: Address = "0x...".parse()?;
//! let vault_apy = client.apy(vault_addr).await?;
//!
//! println!("Vault APY: {:.2}%", vault_apy.apy() * 100.0);
//! println!("Gross APY: {:.2}%", vault_apy.gross_apy() * 100.0);
//! println!("Fee: {:.2}%", vault_apy.fee * 100.0);
//! # Ok(())
//! # }
//! ```

use alloy::{
    primitives::{Address, FixedBytes, U256},
    providers::Provider,
    transports::TransportError,
};
use num_traits::{FromPrimitive, NumOps, One, ToPrimitive};

use crate::hyperevm::{
    DynProvider,
    morpho::contracts::{
        IIrm,
        IMetaMorpho::{self, IMetaMorphoInstance},
        IMorpho::{self, IMorphoInstance},
        Market, MarketParams,
    },
};

pub mod contracts;

/// Morpho market identifier.
///
/// A 32-byte unique identifier for a Morpho Blue market.
pub type MarketId = FixedBytes<32>;

/// Annual Percentage Yield (APY) for a Morpho market.
///
/// Contains both borrow and supply APY rates for a lending market.
///
/// # Example
///
/// Query APY for a market: `client.apy(morpho_addr, market_id).await?`
/// Access borrow and supply APY via `apy.borrow` and `apy.supply` fields.
#[derive(Debug, Clone)]
pub struct PoolApy<T128> {
    /// Market parameters (loan token, collateral, oracle, IRM, LLTV)
    pub params: MarketParams,
    /// Current market state (supply, borrow, fees)
    pub market: Market,
    /// Borrow APY as a decimal (0.05 = 5%)
    pub borrow: T128,
    /// Supply APY as a decimal (0.03 = 3%)
    pub supply: T128,
}

/// MetaMorpho vault APY information.
///
/// A MetaMorpho vault aggregates multiple Morpho markets to optimize yields.
/// This struct contains all the information needed to calculate the vault's APY.
///
/// # Example
///
/// Query vault APY: `client.apy(vault_addr).await?`
/// Calculate effective APY after fees using `vault_apy.apy()` method.
/// Individual market data available in `vault_apy.components`.
#[derive(Debug, Clone)]
pub struct VaultApy<T128> {
    /// Individual markets that compose this vault
    pub components: Vec<VaultSupply<T128>>,
    /// Vault management fee (raw U256 value, divide by 1e18)
    pub fee: U256,
    /// Total assets deposited into the vault (raw U256 value)
    pub total_deposits: U256,
}

#[derive(Debug, Clone)]
pub struct VaultSupply<T128> {
    pub supplied_shares: U256,
    pub pool: PoolApy<T128>,
    /// Supply APY as U256 (scaled by 1e18, e.g., 0.05 * 1e18 = 5% APY)
    pub supply_apy: T128,
}

impl<T128> VaultApy<T128>
where
    T128: ToPrimitive,
{
    /// Calculates the effective vault APY after fees.
    ///
    /// This is a weighted average of all underlying market APYs, adjusted for
    /// the vault's management fee.
    ///
    /// The return value is scaled to 18 decimals. Which means, if the
    /// APY of a vault is 4.20% the string representation of your decimal
    /// implementation should be close to `0.042`.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The numeric type to use for calculations (e.g., f64, Decimal, etc.)
    ///   Must support arithmetic operations and conversion from U256.
    /// - `F`: Conversion function from U256 to T
    ///
    /// # Arguments
    ///
    /// - `convert`: Function to convert U256 values to your numeric type
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hyperevm::morpho;
    /// use hypersdk::hyperevm::DynProvider;
    /// use hypersdk::Address;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let provider = DynProvider::new(hypersdk::hyperevm::mainnet().await?);
    /// let client = morpho::MetaClient::new(provider);
    /// let vault_addr: Address = "0x...".parse()?;
    /// let vault_apy = client.apy_generic::<f64, _>(vault_addr, |value| value.exp()).await?;
    ///
    /// // Using f64
    /// let apy_f64 = vault_apy.apy(|u| u.to::<u128>() as f64);
    ///
    /// // Using a custom Decimal type (rust_decimal example)
    /// // let apy_decimal = vault_apy.apy(|u| Decimal::from_u128(u.to::<u128>()).unwrap());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn apy<T256, F>(&self, convert: F) -> T256
    where
        T256: NumOps + Copy,
        F: Fn(U256) -> T256,
    {
        let zero = convert(U256::ZERO);
        let wad = convert(U256::from(1_000_000_000_000_000_000u128));

        if self.total_deposits.is_zero() {
            return zero;
        }

        let total_deposits = convert(self.total_deposits);
        let fee = convert(self.fee);

        let fee_multiplier = (wad - fee) / wad;

        let gross_apy = self
            .components
            .iter()
            .map(|component| {
                let supplied_shares = convert(component.supplied_shares);
                let total_supply_assets =
                    convert(U256::from(component.pool.market.totalSupplyAssets));
                let total_supply_shares =
                    convert(U256::from(component.pool.market.totalSupplyShares));

                // Convert shares to assets: shares * total_assets / total_shares = assets
                let supplied_assets = supplied_shares * total_supply_assets / total_supply_shares;
                let supply_apy = convert(U256::from(component.supply_apy.to_u128().unwrap()));
                supplied_assets * supply_apy / total_deposits
            })
            .fold(zero, |acc, x| acc + x);

        gross_apy * fee_multiplier / wad
    }

    /// Returns the number of markets in the vault.
    #[must_use]
    pub fn market_count(&self) -> usize {
        self.components.len()
    }
}

/// Client for Morpho Blue lending markets.
///
/// Provides methods for querying market information and calculating APYs.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hyperevm::morpho;
/// use hypersdk::Address;
///
/// # async fn example() -> anyhow::Result<()> {
/// // Create a mainnet client
/// let client = morpho::Client::mainnet().await?;
///
/// // Query a market's APY
/// let morpho_addr: Address = "0x...".parse()?;
/// let market_id = [0u8; 32].into();
/// let apy = client.apy::<f64, _>(morpho_addr, market_id, |e| e.exp()).await?;
///
/// println!("Supply APY: {:.2}%", apy.supply * 100.0);
/// # Ok(())
/// # }
/// ```
pub struct Client<P>
where
    P: Provider,
{
    provider: P,
}

impl Client<DynProvider> {
    /// Creates a client for HyperEVM mainnet.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hyperevm::morpho;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = morpho::Client::mainnet().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn mainnet() -> Result<Self, TransportError> {
        let provider = DynProvider::new(super::mainnet().await?);
        Ok(Self::new(provider))
    }

    /// Creates a client with a custom RPC URL.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hyperevm::morpho;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = morpho::Client::mainnet_with_url("https://custom-rpc.example.com").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn mainnet_with_url(url: &str) -> Result<Self, TransportError> {
        let provider = DynProvider::new(super::mainnet_with_url(url).await?);
        Ok(Self::new(provider))
    }
}

impl<P> Client<P>
where
    P: Provider + Clone,
{
    /// Creates a new Morpho client with a custom provider.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hyperevm::{self, morpho};
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let provider = hyperevm::mainnet().await?;
    /// let client = morpho::Client::new(provider);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    /// Returns a reference to the underlying provider.
    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Creates a Morpho contract instance at the given address.
    ///
    /// Use this to call Morpho contract methods directly.
    pub fn instance(&self, address: Address) -> IMorphoInstance<P> {
        IMorpho::new(address, self.provider.clone())
    }

    /// Calculates the APY for a specific Morpho market.
    ///
    /// This helper types require a type that can handle big decimal conversions
    /// for precision. Using a simple f64 might overflow, cause conversion errors
    /// or be unprecise enough.
    ///
    /// You can still use f64.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hyperevm::{DynProvider, morpho};
    /// use hypersdk::Address;
    ///
    /// async fn example(provider: DynProvider, morpho_addr: Address, market_id: morpho::MarketId) -> anyhow::Result<()> {
    ///     let pool = morpho::Client::new(provider)
    ///         .apy::<f64, _>(morpho_addr, market_id, |e| e.exp())
    ///         .await?;
    ///
    ///     println!("Borrow APY: {:.2}%", pool.borrow * 100.0);
    ///     println!("Supply APY: {:.2}%", pool.supply * 100.0);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn apy<T128, F>(
        &self,
        address: Address,
        market_id: MarketId,
        exp: F,
    ) -> anyhow::Result<PoolApy<T128>>
    where
        T128: FromPrimitive + NumOps + One + Copy,
        F: FnOnce(T128) -> T128,
    {
        let morpho = IMorpho::new(address, self.provider.clone());
        let (params, market) = self
            .provider
            .multicall()
            .add(morpho.idToMarketParams(market_id))
            .add(morpho.market(market_id))
            .aggregate()
            .await?;
        self.apy_with(params, market, exp).await
    }

    /// Returns the APY of the market.
    pub async fn apy_with<T128, F>(
        &self,
        params: impl Into<MarketParams>,
        market: impl Into<Market>,
        exp: F,
    ) -> anyhow::Result<PoolApy<T128>>
    where
        T128: FromPrimitive + NumOps + One + Copy,
        F: FnOnce(T128) -> T128,
    {
        let params = params.into();
        let market = market.into();
        let irm = IIrm::new(params.irm, self.provider.clone());
        let rate = irm
            .borrowRateView(params.into(), market.into())
            .call()
            .await?;

        let error = || anyhow::anyhow!("unable to convert u128 into Float");

        let wad = T128::from_u128(1_000_000_000_000_000_000u128).ok_or_else(error)?;
        let seconds_in_a_year = T128::from_u128(31_536_000).ok_or_else(error)?;
        let one = T128::one();

        let fee = T128::from_u128(market.fee).ok_or_else(error)? / wad;
        let utilization = T128::from_u128(market.totalBorrowAssets).ok_or_else(error)?
            / T128::from_u128(market.totalSupplyAssets).ok_or_else(error)?;
        let rate = T128::from_u128(rate.to::<u128>()).ok_or_else(error)? / wad;
        let borrow_apy = (exp)(rate * seconds_in_a_year) - one;
        let supply_apy = borrow_apy * utilization * (one - fee);

        Ok(PoolApy {
            params,
            market,
            borrow: borrow_apy,
            supply: supply_apy,
        })
    }
}

/// MetaMorpho client
pub struct MetaClient<P>
where
    P: Provider,
{
    provider: P,
}

impl MetaClient<DynProvider> {
    /// Creates a client for mainnet.
    pub async fn mainnet() -> Result<Self, TransportError> {
        let provider = DynProvider::new(super::mainnet().await?);
        Ok(Self::new(provider))
    }

    /// Creates a client for mainnet.
    pub async fn mainnet_with_url(url: &str) -> Result<Self, TransportError> {
        let provider = DynProvider::new(super::mainnet_with_url(url).await?);
        Ok(Self::new(provider))
    }
}

impl<P> MetaClient<P>
where
    P: Provider + Clone,
{
    /// Create a uniswap client.
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    /// Returns the root provider.
    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Creates a MetaMorphoInstance.
    pub fn instance(&self, address: Address) -> IMetaMorphoInstance<P> {
        IMetaMorpho::new(address, self.provider.clone())
    }

    /// Returns the pool's APY with custom numeric types.
    ///
    /// For a simpler interface, use `apy()` or `apy_simple()` which return `SimpleVaultApy`.
    ///
    /// <https://github.com/morpho-org/metamorpho-v1.1/blob/main/src/MetaMorphoV1_1.sol#L796>
    pub async fn apy_generic<T128, F>(&self, address: Address, exp: F) -> anyhow::Result<VaultApy<T128>>
    where
        T128: FromPrimitive + NumOps + One + Copy,
        F: FnOnce(T128) -> T128 + Copy,
    {
        let error = || anyhow::anyhow!("unable to convert u128 into Float");
        let wad = T128::from_u128(1_000_000_000_000_000_000u128).ok_or_else(error)?;

        let meta_morpho = IMetaMorpho::new(address, self.provider.clone());
        let (fee, supply_queue_len, total_assets, morpho_addr) = self
            .provider
            .multicall()
            .add(meta_morpho.fee())
            .add(meta_morpho.supplyQueueLength())
            .add(meta_morpho.totalAssets())
            .add(meta_morpho.MORPHO())
            .aggregate()
            .await?;
        let supply_queue_len = supply_queue_len.to::<usize>();

        let morpho = IMorpho::new(morpho_addr, self.provider.clone());

        let mut apy = VaultApy {
            components: vec![],
            fee: U256::from(fee),
            total_deposits: total_assets,
        };
        for i in 0..supply_queue_len {
            // TODO: is there a way to aggregate this?
            let market_id = meta_morpho.supplyQueue(U256::from(i)).call().await?;

            let (config, params, market) = self
                .provider
                .multicall()
                .add(meta_morpho.config(market_id))
                .add(morpho.idToMarketParams(market_id))
                .add(morpho.market(market_id))
                .aggregate()
                .await?;

            if !config.enabled
                || params.irm.is_zero()
                || params.collateralToken.is_zero()
                || params.loanToken.is_zero()
            {
                // println!("{} has no IRM?", market_id);
                continue;
            }

            let position = morpho
                .position(market_id, *meta_morpho.address())
                .call()
                .await?;

            let pool = Client::new(self.provider.clone())
                .apy_with::<T128, F>(params, market, exp)
                .await?;

            let supply_apy = pool.supply * wad;

            apy.components.push(VaultSupply {
                supplied_shares: position.supplyShares,
                pool,
                supply_apy,
            });
        }

        Ok(apy)
    }

    /// Returns the pool's APY as a simple f64-based struct.
    ///
    /// This is a convenience method that handles all the numeric conversions internally.
    /// The returned struct provides easy access to APY, fees, and deposits as f64 values.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hyperevm::morpho;
    /// use hypersdk::Address;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = morpho::MetaClient::mainnet().await?;
    /// let vault_addr: Address = "0x...".parse()?;
    ///
    /// let vault_apy = client.apy_simple(vault_addr).await?;
    /// println!("APY: {:.2}%", vault_apy.apy() * 100.0);
    /// println!("Gross APY: {:.2}%", vault_apy.gross_apy() * 100.0);
    /// println!("Fee: {:.2}%", vault_apy.fee * 100.0);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn apy_simple(&self, address: Address) -> anyhow::Result<SimpleVaultApy> {
        let vault = self.apy_generic::<f64, _>(address, |x| x.exp()).await?;
        Ok(SimpleVaultApy::from_vault_apy(vault))
    }
}

impl MetaClient<DynProvider> {
    /// Returns the pool's APY as a simple f64-based struct.
    ///
    /// This is a convenience method that handles all the numeric conversions internally.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hyperevm::morpho;
    /// use hypersdk::Address;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = morpho::MetaClient::mainnet().await?;
    /// let vault_addr: Address = "0x...".parse()?;
    ///
    /// let vault_apy = client.apy(vault_addr).await?;
    /// println!("APY: {:.2}%", vault_apy.apy() * 100.0);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn apy(&self, address: Address) -> anyhow::Result<SimpleVaultApy> {
        self.apy_simple(address).await
    }
}

/// Simplified vault APY struct with f64 values.
///
/// This struct provides a convenient interface for accessing vault APY data
/// without needing to handle complex numeric types or conversions.
#[derive(Debug, Clone)]
pub struct SimpleVaultApy {
    /// Vault management fee as a decimal (0.05 = 5%).
    pub fee: f64,
    /// Total deposits in the vault (in base units).
    pub total_deposits: f64,
    /// Number of markets in the vault.
    pub market_count: usize,
    /// Gross APY before fees (as a decimal, 0.05 = 5%).
    gross_apy: f64,
}

impl SimpleVaultApy {
    /// Creates a SimpleVaultApy from a VaultApy<f64>.
    fn from_vault_apy(vault: VaultApy<f64>) -> Self {
        const WAD: f64 = 1e18;

        let fee = vault.fee.to::<u128>() as f64 / WAD;
        let total_deposits = vault.total_deposits.to::<u128>() as f64 / WAD;

        // Calculate gross APY (weighted average of all markets)
        let gross_apy = if vault.total_deposits.is_zero() {
            0.0
        } else {
            vault
                .components
                .iter()
                .map(|c| {
                    let supplied_shares = c.supplied_shares.to::<u128>() as f64;
                    let total_supply_assets = c.pool.market.totalSupplyAssets as f64;
                    let total_supply_shares = c.pool.market.totalSupplyShares as f64;

                    if total_supply_shares == 0.0 {
                        return 0.0;
                    }

                    let supplied_assets = supplied_shares * total_supply_assets / total_supply_shares;
                    let supply_apy = c.supply_apy / WAD;
                    supplied_assets * supply_apy / (total_deposits * WAD)
                })
                .sum()
        };

        Self {
            fee,
            total_deposits,
            market_count: vault.components.len(),
            gross_apy,
        }
    }

    /// Returns the effective APY after fees.
    ///
    /// This is the net yield a depositor would receive, accounting for the
    /// vault's management fee.
    #[must_use]
    pub fn apy(&self) -> f64 {
        self.gross_apy * (1.0 - self.fee)
    }

    /// Returns the gross APY before fees.
    ///
    /// This is the raw yield generated by the underlying markets before
    /// the vault's management fee is applied.
    #[must_use]
    pub fn gross_apy(&self) -> f64 {
        self.gross_apy
    }

    /// Returns the number of markets in the vault.
    #[must_use]
    pub fn market_count(&self) -> usize {
        self.market_count
    }
}
