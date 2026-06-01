//! Split and merge outcome shares on a HIP-4 outcome market.
//!
//! Demonstrates the full split → wait → merge lifecycle:
//! 1. **Split** — lock collateral (USDC) to mint YES + NO shares
//! 2. Wait 10 seconds
//! 3. **Merge** — burn both shares to reclaim the collateral
//!
//! Outcome contracts always satisfy `YES_price + NO_price == 1`, so splitting
//! is a neutral operation — you receive shares worth exactly your collateral.
//! Merging reverses it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::Parser;
use hypersdk::hypercore;

use crate::credentials::Credentials;

mod credentials;

#[derive(Parser, Debug, derive_more::Deref)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[deref]
    #[command(flatten)]
    common: Credentials,
    /// Outcome ID to operate on (from outcomeMeta info endpoint).
    #[arg(short, long)]
    outcome: u32,
    /// Amount of USDC to lock, in wei (1 USDC = 1e6 wei for most outcomes).
    /// E.g. 1_000_000 = $1.00
    #[arg(short, long)]
    wei: u64,
}

fn nonce() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = simple_logger::init_with_level(log::Level::Debug);

    let args = Cli::parse();
    let signer = args.get()?;

    let client = hypercore::mainnet();

    println!(
        "Account: {}\nOutcome: {} | Wei: {}",
        signer.address(),
        args.outcome,
        args.wei
    );

    // --- Step 1: Split ---
    println!("\n[1/3] Splitting — locking collateral to mint YES + NO shares...");
    let resp = client
        .split_outcome(&signer, args.outcome, args.wei, nonce())
        .await?;
    println!("Split response: {resp:?}");

    // --- Step 2: Wait ---
    println!("\n[2/3] Waiting 10 seconds before merge...");
    tokio::time::sleep(Duration::from_secs(10)).await;

    // --- Step 3: Merge ---
    println!("\n[3/3] Merging — burning YES + NO to reclaim collateral...");
    let resp = client
        .merge_outcome(&signer, args.outcome, args.wei, nonce())
        .await?;
    println!("Merge response: {resp:?}");

    println!("\nDone!");
    Ok(())
}
