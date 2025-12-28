use std::{
    env,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use hypersdk::hypercore::{self as hypercore};
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

    dotenvy::dotenv()?;
    let private_key = env::var("PRIVATE_KEY")?;
    let signer = hypercore::PrivateKeySigner::from_str(&private_key)?;

    let _ = simple_logger::init_with_level(log::Level::Debug);

    let client = hypercore::mainnet();

    let tokens = client.spot_tokens().await?;
    let token = tokens
        .iter()
        .find(|token| token.name == args.token)
        .ok_or(anyhow::anyhow!("{} not found", args.token))?
        .clone();

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    client
        .transfer_to_evm(&signer, token.clone(), args.amount, nonce)
        .await?;

    Ok(())
}
