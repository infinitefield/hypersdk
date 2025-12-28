use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use hypersdk::hypercore::{self as hypercore};
use rust_decimal::Decimal;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Private key.
    #[arg(short, long)]
    private_key: String,
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
        .transfer_to_spot(&signer, token.clone(), args.amount, nonce)
        .await?;

    Ok(())
}
