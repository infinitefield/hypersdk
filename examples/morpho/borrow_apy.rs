use alloy::{primitives::FixedBytes, providers::Provider};
use chrono::Utc;
use clap::Parser;
use hypersdk::{
    Address,
    hyperevm::{self, DynProvider, ERC20},
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    println!("Connecting to RPC endpoint: {}", args.rpc_url);

    let provider = DynProvider::new(hyperevm::mainnet_with_url(&args.rpc_url).await?);
    let morpho = hyperevm::morpho::Client::new(provider.clone());
    let apy = morpho.apy(args.contract_address, args.market_id).await?;

    let last_update =
        chrono::DateTime::<Utc>::from_timestamp_secs(apy.market.lastUpdate as i64).unwrap();
    println!("market params last updated at {}", last_update);

    let market_params = &apy.params;
    let (collateral, loan) = provider
        .multicall()
        .add(ERC20::new(market_params.collateralToken, provider.clone()).symbol())
        .add(ERC20::new(market_params.loanToken, provider.clone()).symbol())
        .aggregate()
        .await?;

    println!(
        "borrow APY for {loan} / {collateral} is {}",
        apy.borrow * 100.0
    );

    Ok(())
}
