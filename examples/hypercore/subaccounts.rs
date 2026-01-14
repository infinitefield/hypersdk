//! List subaccounts for a master account on Hyperliquid.
//!
//! This example demonstrates how to retrieve all subaccounts associated with a
//! master account, including their clearinghouse state and spot balances.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example subaccounts -- <MASTER_ADDRESS>
//! ```

use clap::Parser;
use hypersdk::{Address, hypercore};

#[derive(Parser)]
struct Args {
    /// Master account address to query subaccounts for
    master: Address,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let client = hypercore::mainnet();

    let subaccounts = client.subaccounts(args.master).await?;

    if subaccounts.is_empty() {
        println!("No subaccounts found for {:?}", args.master);
        return Ok(());
    }

    println!("Subaccounts for {:?}:", args.master);
    println!();

    for sub in &subaccounts {
        println!("Name: {}", sub.name);
        println!("  Address: {:?}", sub.sub_account_user);
        println!("  Master: {:?}", sub.master);
        println!();

        // Clearinghouse state (perpetuals)
        let margin = &sub.clearinghouse_state.margin_summary;
        println!("  Perpetuals:");
        println!("    Account Value: ${}", margin.account_value);
        println!("    Total Position: ${}", margin.total_ntl_pos);
        println!("    Margin Used: ${}", margin.total_margin_used);
        println!(
            "    Withdrawable: ${}",
            sub.clearinghouse_state.withdrawable
        );

        // Open positions
        if !sub.clearinghouse_state.asset_positions.is_empty() {
            println!("    Positions:");
            for pos in &sub.clearinghouse_state.asset_positions {
                let p = &pos.position;
                println!(
                    "      {} {}: {} @ {:?} (PnL: ${})",
                    if p.is_long() { "LONG" } else { "SHORT" },
                    p.coin,
                    p.szi.abs(),
                    p.entry_px,
                    p.unrealized_pnl
                );
            }
        }

        // Spot balances
        if !sub.spot_state.balances.is_empty() {
            println!();
            println!("  Spot Balances:");
            for balance in &sub.spot_state.balances {
                if !balance.total.is_zero() {
                    println!(
                        "    {}: {} (held: {})",
                        balance.coin, balance.total, balance.hold
                    );
                }
            }
        }

        println!();
        println!("---");
        println!();
    }

    println!("Total subaccounts: {}", subaccounts.len());

    Ok(())
}
