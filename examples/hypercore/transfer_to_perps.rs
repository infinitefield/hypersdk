//! Transfer USDC from the exchange spot wallet to the perps wallet.
//!
//! Demonstrates moving USDC between internal exchange balances, specifically from the
//! spot vault into the perpetuals vault for margin on derivative positions.

use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use hypersdk::hypercore::{self as hypercore};
use rust_decimal::Decimal;

use crate::credentials::Credentials;

mod credentials;

#[derive(Parser, Debug, derive_more::Deref)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[deref]
    #[command(flatten)]
    common: Credentials,
    /// Amount to send
    #[arg(short, long)]
    amount: Decimal,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = simple_logger::init_with_level(log::Level::Debug);

    let args = Cli::parse();
    let signer = args.get()?;

    let client = hypercore::mainnet();

    let tokens = client.spot_tokens().await?;
    let token = tokens
        .iter()
        .find(|token| token.name == "USDC")
        .unwrap()
        .clone();

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    client
        .transfer_to_perps(&signer, token.clone(), args.amount, nonce)
        .await?;

    Ok(())
}
