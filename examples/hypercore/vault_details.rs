//! Query details for a Hyperliquid vault.
//!
//! This example demonstrates how to retrieve comprehensive information about a vault
//! including its performance metrics, followers, and configuration.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example vault_details -- <VAULT_ADDRESS>
//! ```
//!
//! # Example
//!
//! ```bash
//! cargo run --example vault_details -- 0xdfc24b077bc1425ad1dea75bcb6f8158e10df303
//! ```

use clap::Parser;
use hypersdk::{Address, hypercore};
use rust_decimal::Decimal;

#[derive(Parser)]
struct Args {
    /// Vault address to query
    vault: Address,
    /// Optional user address to include follower state
    #[arg(short, long)]
    user: Option<Address>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let client = hypercore::mainnet();

    let details = client.vault_details(args.vault, args.user).await?;

    println!("Vault: {}", details.name);
    println!("Address: {:?}", details.vault_address);
    println!("Leader: {:?}", details.leader);
    println!("Description: {}", details.description);
    println!();
    println!("APR: {}%", details.apr * Decimal::ONE_HUNDRED);
    println!(
        "Leader Fraction: {}%",
        details.leader_fraction * Decimal::ONE_HUNDRED
    );
    println!(
        "Leader Commission: {}%",
        details.leader_commission * Decimal::ONE_HUNDRED
    );
    println!("Max Distributable: ${}", details.max_distributable);
    println!("Max Withdrawable: ${}", details.max_withdrawable);
    println!();
    println!("Followers: {}", details.followers.len());

    // Show top 5 followers by equity
    let mut followers = details.followers.clone();
    followers.sort_by(|a, b| b.vault_equity.cmp(&a.vault_equity));
    for (i, follower) in followers.iter().take(5).enumerate() {
        println!(
            "  {}. {}: ${} (PnL: ${})",
            i + 1,
            follower.user,
            follower.vault_equity,
            follower.pnl
        );
    }

    // Show user's follower state if provided
    if let Some(state) = details.follower_state {
        println!();
        println!("Your Position:");
        println!("  Equity: ${}", state.vault_equity);
        println!("  PnL: ${}", state.pnl);
        println!("  All-time PnL: ${}", state.all_time_pnl);
        println!("  Days Following: {}", state.days_following);
        if let Some(lockup) = state.lockup_until {
            println!("  Locked Until: {}", lockup);
        }
    }

    // Show portfolio performance for different time periods
    println!();
    println!("Portfolio Performance:");
    for (period, portfolio) in &details.portfolio {
        if !portfolio.account_value_history.is_empty() {
            let latest_value = &portfolio.account_value_history.last().unwrap().1;
            println!("  {}: ${} (vlm: ${})", period, latest_value, portfolio.vlm);
        }
    }

    Ok(())
}
