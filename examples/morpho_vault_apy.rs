use alloy::{providers::Provider, sol};
use clap::Parser;
use hypersdk::{
    Address, U256,
    hyperevm::{self, DynProvider, IERC20},
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Address of the vault contract.
    #[arg(
        short,
        long,
        default_value = "0x207ccaE51Ad2E1C240C4Ab4c94b670D438d2201C"
    )]
    contract_address: Address,
    /// RPC url
    #[arg(short, long, default_value = "http://127.0.0.1:8545")]
    rpc_url: String,
}

sol! {
    struct MarketConfig {
        uint184 cap;
        bool enabled;
        uint64 removableAt;
    }

    struct MarketParams {
        address loanToken;
        address collateralToken;
        address oracle;
        address irm;
        uint256 lltv;
    }

    struct Market {
        uint128 totalSupplyAssets;
        uint128 totalSupplyShares;
        uint128 totalBorrowAssets;
        uint128 totalBorrowShares;
        uint128 lastUpdate;
        uint128 fee;
    }

    #[sol(rpc)]
    contract Morpho {
        function market(bytes32 market) returns (Market);
        function idToMarketParams(bytes32 market) returns (MarketParams);
        function position(bytes32 id, address user)
                external
                view
                returns (uint256 supplyShares, uint128 borrowShares, uint128 collateral);
    }

    #[sol(rpc)]
    contract MetaMorpho {
        bytes32[] public supplyQueue;

        function MORPHO() external view returns (address);
        function fee() returns (uint96);
        function supplyQueueLength() external view returns (uint256);
        function config(bytes32 market) returns (MarketConfig);
    }

    #[sol(rpc)]
    contract AdaptativeCurveIrm {
        type Id is bytes32;

        function MORPHO() external view returns (address);
        function borrowRateView(MarketParams memory marketParams, Market memory market) external returns (uint256);
    }
}

// https://github.com/morpho-org/metamorpho-v1.1/blob/main/src/MetaMorphoV1_1.sol#L796

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    println!("Connecting to RPC endpoint: {}", args.rpc_url);

    let provider = DynProvider::new(hyperevm::mainnet_with_url(&args.rpc_url).await?);

    let meta_morpho = MetaMorpho::new(args.contract_address, provider.clone());
    let fee = meta_morpho.fee().call().await?.to::<u64>() as f64 / 1e18;
    let morpho = Morpho::new(meta_morpho.MORPHO().call().await?, provider.clone());
    let supply_queue_len = meta_morpho.supplyQueueLength().call().await?.to::<usize>();

    let vault_erc20 = IERC20::new(args.contract_address, provider.clone());
    let total_deposits =
        (vault_erc20.totalSupply().call().await? / U256::from(1e18)).to::<u64>() as f64;

    let mut apy = 0.0;
    for i in 0..supply_queue_len {
        let market_id = meta_morpho.supplyQueue(U256::from(i)).call().await?;
        let (config, params, market) = provider
            .multicall()
            .add(meta_morpho.config(market_id))
            .add(morpho.idToMarketParams(market_id))
            .add(morpho.market(market_id))
            .aggregate()
            .await?;

        if params.irm.is_zero() || params.collateralToken.is_zero() || params.loanToken.is_zero() {
            println!("{} has no IRM?", market_id);
            continue;
        }

        let (collateral, loan) = provider
            .multicall()
            .add(IERC20::new(params.collateralToken, provider.clone()).symbol())
            .add(IERC20::new(params.loanToken, provider.clone()).symbol())
            .aggregate()
            .await?;

        let utilization = market.totalBorrowAssets as f64 / market.totalSupplyAssets as f64;
        let fee = market.fee as f64 / 1e18;
        if !config.enabled {
            continue;
        }

        let position = morpho
            .position(market_id, *meta_morpho.address())
            .call()
            .await?;

        let irm = AdaptativeCurveIrm::new(params.irm, provider.clone());
        let rate = irm.borrowRateView(params, market.clone()).call().await?;

        // https://github.com/morpho-org/morpho-blue/blob/48b2a62d9d911a27f886fb7909ad57e29f7dacc9/src/libraries/SharesMathLib.sol#L20
        let supplied_shares = (position.supplyShares / U256::from(1e6)).to::<u64>() as f64;
        let supplied_assets =
            // get the price per share * supplied_shares
            (market.totalSupplyAssets as f64 / market.totalSupplyShares as f64) * supplied_shares;
        let total_assets = market.totalSupplyAssets as f64;

        let rate = rate.to::<u64>() as f64 / 1e18;

        let borrow_apy = (rate * 31_536_000f64).exp() - 1.0;
        let supply_apy = borrow_apy * utilization * (1.0 - fee);

        println!(
            "{loan} / {collateral}: supplied_assets={}, total_assets={}, allocated={}, utilization={}",
            supplied_assets,
            total_assets,
            supplied_assets / total_deposits * 100.0,
            utilization
        );

        apy += supplied_assets * supply_apy / total_deposits;
    }

    println!("apy: {}", apy * (1.0 - fee) * 100.0);

    Ok(())
}
