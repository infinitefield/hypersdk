//! Query the maximum builder fee a user has approved for a builder on Hyperliquid.
//!
//! The value is expressed in tenths of a basis point (e.g. `1` means 0.001%).
//!
//! # Usage
//!
//! ```bash
//! cargo run --example max_builder_fee -- <USER_ADDRESS> <BUILDER_ADDRESS>
//! ```

use clap::Parser;
use hypersdk::{Address, hypercore};

#[derive(Parser)]
struct Args {
    /// User address to query the approved fee for
    user: Address,
    /// Builder address to check approval against
    builder: Address,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = hypercore::mainnet();

    let max_fee = client.max_builder_fee(args.user, args.builder).await?;

    println!("User:    {:?}", args.user);
    println!("Builder: {:?}", args.builder);
    println!(
        "Max approved builder fee: {} (tenths of a bps, e.g. 1 = 0.001%)",
        max_fee
    );

    Ok(())
}
