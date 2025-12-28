use std::{env, str::FromStr};

use alloy::{network::TransactionBuilder, rpc::types::TransactionRequest};
use clap::Parser;
use hypersdk::{
    hypercore::{self as hypercore},
    hyperevm::{self, ProviderTrait},
};
use rust_decimal::Decimal;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Token to transfer
    #[arg(short, long)]
    token: String,
    /// Amount to send
    #[arg(short, long)]
    amount: Decimal,
    /// Amount to send
    #[arg(short, long, default_value = "https://rpc.hyperliquid.xyz/evm")]
    rpc_url: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    let _ = simple_logger::init_with_level(log::Level::Debug);

    dotenvy::dotenv()?;
    let private_key = env::var("PRIVATE_KEY")?;
    let signer = hypercore::PrivateKeySigner::from_str(&private_key)?;

    log::info!("Signer address: {}", signer.address());

    let client = hypercore::mainnet();

    let tokens = client.spot_tokens().await?;
    let token = tokens
        .iter()
        .find(|token| token.name == args.token)
        .ok_or(anyhow::anyhow!("{} not found", args.token))?
        .clone();

    let wei = token.to_wei(args.amount);
    let send_to = token.cross_chain_address.as_ref().unwrap();
    log::info!("Sending {} ({wei}) to {}", args.amount, send_to);

    let provider = hyperevm::mainnet_with_signer_and_url(&args.rpc_url, signer).await?;
    let tx = TransactionRequest::default()
        .with_to(*send_to)
        .with_value(wei);

    let pending = provider
        .send_transaction(tx)
        .await
        .map_err(|err| anyhow::anyhow!("send tx: {err}"))?;
    log::info!("Sent {}", pending.tx_hash());

    let receipt = pending.get_receipt().await?;
    log::info!("receipt: {receipt:?}");

    Ok(())
}
