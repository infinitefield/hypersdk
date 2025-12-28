use alloy::{primitives::FixedBytes, sol};
use chrono::Utc;
use clap::Parser;
use hypersdk::{
    Address,
    hyperevm::{self, DynProvider},
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Address of the IRM contract.
    #[arg(
        short,
        long,
        default_value = "0xD4a426F010986dCad727e8dd6eed44cA4A9b7483"
    )]
    contract_address: Address,
    // Morpho market
    #[arg(short, long)]
    market_id: FixedBytes<32>,
    /// RPC url
    #[arg(short, long, default_value = "http://127.0.0.1:8545")]
    rpc_url: String,
}

sol! {
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
    }

    #[sol(rpc)]
    contract AdaptativeCurveIrm {
        type Id is bytes32;

        function MORPHO() external view returns (address);
        function borrowRateView(MarketParams memory marketParams, Market memory market) external returns (uint256);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    println!("Connecting to RPC endpoint: {}", args.rpc_url);

    let provider = DynProvider::new(hyperevm::mainnet_with_url(&args.rpc_url).await?);

    let irm = AdaptativeCurveIrm::new(args.contract_address, provider.clone());
    let morpho_address = irm.MORPHO().call().await?;
    println!("Morpho {morpho_address}");

    let morpho = Morpho::new(morpho_address, provider.clone());

    let market = morpho.market(args.market_id).call().await?;
    let last_update =
        chrono::DateTime::<Utc>::from_timestamp_secs(market.lastUpdate as i64).unwrap();
    println!("market params last updated at {}", last_update);

    let market_params = morpho.idToMarketParams(args.market_id).call().await?;

    let utilization = market.totalBorrowAssets as f64 / market.totalSupplyAssets as f64;
    let fee = market.fee as f64 / 1e18;

    let rate = irm.borrowRateView(market_params, market).call().await?;
    println!("borrowing rate is {rate}, utilization {utilization}");

    let rate = rate.to::<u64>() as f64 / 1e18;

    let borrow_apy = (rate * 31_536_000f64).exp() - 1.0;
    let supply_apy = borrow_apy * utilization * (1.0 - fee);
    println!("borrow APY is {}", borrow_apy * 100.0);
    println!("supply APY is {}", supply_apy * 100.0);

    Ok(())
}
