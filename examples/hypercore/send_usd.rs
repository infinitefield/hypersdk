use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use hypersdk::{
    Address,
    hypercore::{
        self as hypercore, ARBITRUM_SIGNATURE_CHAIN_ID,
        types::{HyperliquidChain, UsdSend},
    },
};
use rust_decimal::Decimal;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Private key.
    #[arg(short, long)]
    private_key: String,
    /// Address to send the transfer to.
    #[arg(short, long)]
    to: Address,
    /// Amount to send
    #[arg(short, long)]
    amount: Decimal,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = simple_logger::init_with_level(log::Level::Debug);

    let args = Cli::parse();

    let client = hypercore::mainnet();
    let signer = hypercore::PrivateKeySigner::from_str(&args.private_key)?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    client
        .send_usdc(
            &signer,
            UsdSend {
                hyperliquid_chain: HyperliquidChain::Mainnet,
                signature_chain_id: ARBITRUM_SIGNATURE_CHAIN_ID,
                destination: signer.address(),
                amount: args.amount,
                time: nonce,
            },
            nonce,
        )
        .await?;

    Ok(())
}
