use alloy::{
    primitives::{Address, FixedBytes, U256},
    providers::Provider,
    transports::TransportError,
};

use crate::hyperevm::{
    DynProvider, IERC20,
    morpho::contracts::{AdaptativeCurveIrm, Morpho},
};

pub mod contracts;

/// Pool's APY
#[derive(Debug, Clone)]
pub struct PoolApy {
    /// Market parameters
    pub params: contracts::MarketParams,
    /// Morpho Market
    pub market: contracts::Market,
    /// Borrow APY
    pub borrow: f64,
    /// Supply APY
    pub supply: f64,
}

/// MetaMorpho's vault APY
#[derive(Debug, Clone)]
pub struct VaultApy {
    /// Markets that compose this vault.
    pub components: Vec<VaultSupply>,
    /// Fee
    pub fee: f64,
    /// Total assets deposited into the vault.
    pub total_deposits: f64,
}

#[derive(Debug, Clone)]
pub struct VaultSupply {
    pub supplied_shares: U256,
    pub pool: PoolApy,
}

impl VaultApy {
    /// Returns the MetaMorpho vault APY.
    pub fn apy(&self) -> f64 {
        self.components
            .iter()
            .map(|compontent| {
                // https://github.com/morpho-org/morpho-blue/blob/48b2a62d9d911a27f886fb7909ad57e29f7dacc9/src/libraries/SharesMathLib.sol#L20
                let supplied_shares =
                    (compontent.supplied_shares / U256::from(1e6)).to::<u64>() as f64;
                // to get the supplied assets determine the price per share
                let supplied_assets = (compontent.pool.market.totalSupplyAssets as f64
                    / compontent.pool.market.totalSupplyShares as f64)
                    * supplied_shares;
                supplied_assets * compontent.pool.supply / self.total_deposits
            })
            .sum::<f64>()
            * (1.0 - self.fee)
    }
}

/// Morpho client
pub struct Client<P>
where
    P: Provider,
{
    provider: P,
}

impl Client<DynProvider> {
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

impl<P> Client<P>
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

    /// Returns the pool's APY.
    pub async fn apy(
        &self,
        address: Address,
        market_id: FixedBytes<32>,
    ) -> anyhow::Result<PoolApy> {
        let morpho = contracts::Morpho::new(address, self.provider.clone());
        let (params, market) = self
            .provider
            .multicall()
            .add(morpho.idToMarketParams(market_id))
            .add(morpho.market(market_id))
            .aggregate()
            .await?;
        self.apy_with(params, market).await
    }

    /// Returns the APY.
    pub async fn apy_with(
        &self,
        params: contracts::MarketParams,
        market: contracts::Market,
    ) -> anyhow::Result<PoolApy> {
        let irm = AdaptativeCurveIrm::new(params.irm, self.provider.clone());
        let rate = irm
            .borrowRateView(params.clone(), market.clone())
            .call()
            .await?;

        let fee = market.fee as f64 / 1e18;
        let utilization = market.totalBorrowAssets as f64 / market.totalSupplyAssets as f64;
        let rate = rate.to::<u64>() as f64 / 1e18;
        let borrow_apy = (rate * 31_536_000f64).exp() - 1.0;
        let supply_apy = borrow_apy * utilization * (1.0 - fee);
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

    /// Returns the pool's APY.
    ///
    /// https://github.com/morpho-org/metamorpho-v1.1/blob/main/src/MetaMorphoV1_1.sol#L796
    pub async fn apy(&self, address: Address) -> anyhow::Result<VaultApy> {
        let meta_morpho = contracts::MetaMorpho::new(address, self.provider.clone());
        // the vault is at the same time a token and holds balances
        let vault_erc20 = IERC20::new(address, self.provider.clone());
        let (fee, supply_queue_len, total_supply, morpho_addr) = self
            .provider
            .multicall()
            .add(meta_morpho.fee())
            .add(meta_morpho.supplyQueueLength())
            .add(vault_erc20.totalSupply())
            .add(meta_morpho.MORPHO())
            .aggregate()
            .await?;
        // vault fee
        let fee = fee.to::<u64>() as f64 / 1e18;
        // total deposits in the vault
        let total_deposits = (total_supply / U256::from(1e18)).to::<u64>() as f64;
        let supply_queue_len = supply_queue_len.to::<usize>();

        let morpho = Morpho::new(morpho_addr, self.provider.clone());

        let mut apy = VaultApy {
            components: vec![],
            fee,
            total_deposits,
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
                .apy_with(params, market)
                .await?;

            apy.components.push(VaultSupply {
                supplied_shares: position.supplyShares,
                pool,
            });
        }

        Ok(apy)
    }
}
