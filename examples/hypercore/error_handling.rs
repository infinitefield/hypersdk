//! Demonstrates how to downcast `anyhow::Error` to specific error types
//! for programmatic error handling.

use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use hypersdk::{
    Address,
    hypercore::{self as hypercore, ApiError, types::UsdSend},
};
use rust_decimal::dec;

use crate::credentials::Credentials;

mod credentials;

#[derive(Parser, Debug, derive_more::Deref)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[deref]
    #[command(flatten)]
    common: Credentials,
    #[arg(short, long)]
    to: Address,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = simple_logger::init_with_level(log::Level::Debug);

    let args = Cli::parse();
    let signer = args.get()?;
    let client = hypercore::mainnet();

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let result = client
        .send_usdc(
            &signer,
            UsdSend {
                destination: args.to,
                amount: dec!(1),
                time: nonce,
            },
            nonce,
        )
        .await;

    match result {
        Ok(()) => println!("Transfer succeeded"),
        Err(e) => {
            if let Some(err) = e.downcast_ref::<ApiError>() {
                println!("API rejected: {err}");
            } else if let Some(err) = e.downcast_ref::<reqwest::Error>() {
                if err.is_timeout() {
                    println!("Timed out, safe to retry");
                } else {
                    println!("Network error: {err}");
                }
            } else if let Some(err) = e.downcast_ref::<serde_json::Error>() {
                println!("Bad response JSON: {err}");
            } else {
                println!("Other error: {e}");
            }
        }
    }

    Ok(())
}
