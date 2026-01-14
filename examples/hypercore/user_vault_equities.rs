//! Query a user's vault deposits on Hyperliquid.
//!
//! This example demonstrates how to retrieve all vaults that a user has deposited
//! into, along with their current equity in each vault.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example user_vault_equities -- <USER_ADDRESS>
//! ```

use clap::Parser;
use hypersdk::{Address, hypercore};
use rust_decimal::Decimal;

#[derive(Parser)]
struct Args {
    /// User address to query vault deposits for
    user: Address,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let client = hypercore::mainnet();

    let equities = client.user_vault_equities(args.user).await?;

    if equities.is_empty() {
        println!("User {:?} has no vault deposits", args.user);
        return Ok(());
    }

    println!("Vault deposits for {:?}:", args.user);
    println!();

    let mut total_equity = Decimal::ZERO;

    for equity in &equities {
        println!("Vault: {:?}", equity.vault_address);
        println!("  Equity: ${}", equity.equity);
        if let Some(locked_until) = equity.locked_until_timestamp {
            println!("  Locked until: {}", locked_until);
        }
        println!();
        total_equity += equity.equity;
    }

    println!(
        "Total equity across {} vaults: ${}",
        equities.len(),
        total_equity
    );

    Ok(())
}
