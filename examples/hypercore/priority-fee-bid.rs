//! Bid on the Hyperliquid gossip priority auction.
//!
//! This demonstrates how to query the current auction status and place a signed bid
//! using the SDK's `gossip_priority_bid` method.
//!
//! Usage:
//!     cargo run --example priority-fee-bid -- --max 50 --ip 1.2.3.4
//!
//! --max limits the maximum HYPE bid (converted to wei internally).
//!
//! Note: The fee is deducted from your **spot** HYPE balance and burned.

use clap::Parser;
use hypersdk::{hypercore, hypercore::types::GossipPriorityAuctionStatus, hyperevm};
use rust_decimal::Decimal;

mod credentials;
use credentials::Credentials;

#[derive(clap::Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[command(flatten)]
    credentials: Credentials,
    /// Maximum HYPE to bid (in HYPE, not wei).
    #[arg(long)]
    max: Decimal,
    /// IP address to receive prioritized gossip data.
    #[arg(long)]
    ip: String,
    /// Slot index to bid on (0=highest priority, 4=lowest). Defaults to 0.
    #[arg(long, default_value = "0")]
    slot: u8,
}

fn print_auction_status(status: &GossipPriorityAuctionStatus) {
    println!("\nCurrent auction status (180-second cycle):");
    println!(
        "{:<6} {:>12} {:>14} {:>12} {:>10}",
        "Slot", "Start Gas", "Current Gas", "End/Min", "Time Left"
    );
    println!("{}", "-".repeat(55));

    let now = chrono::Utc::now().timestamp() as u64;

    for (i, slot) in status.iter().enumerate() {
        let elapsed = now.saturating_sub(slot.start_time_seconds);
        let progress = (elapsed as f64 / slot.duration_seconds as f64).clamp(0.0, 1.0);

        let start: Decimal = slot.start_gas.parse().unwrap_or_default();
        let end: Decimal = slot
            .end_gas
            .as_ref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(start);
        let current_price =
            start - (start - end) * Decimal::from_f64_retain(progress).unwrap_or_default();

        let secs_left = slot
            .start_time_seconds
            .saturating_add(slot.duration_seconds)
            .saturating_sub(now);

        let current_str = if slot.current_gas.is_some() {
            format!("{:.4}", current_price)
        } else {
            "(no bid)".to_string()
        };

        println!(
            "{:<6} {:>12} {:>14} {:>12} {:>10}s",
            i,
            slot.start_gas,
            current_str,
            slot.end_gas.as_deref().unwrap_or("-"),
            secs_left
        );
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = simple_logger::init_with_level(log::Level::Info);

    let args = Cli::parse();
    let signer = args.credentials.get()?;
    let client = hypercore::mainnet();
    let nonce = chrono::Utc::now().timestamp_millis() as u64;

    // 1. Query and display current auction status.
    println!("Fetching gossip priority auction status...");
    let status = client.gossip_priority_auction_status().await?;
    print_auction_status(&status);

    // 2. Determine HYPE decimals dynamically from the network.
    let decimals = client
        .spot_tokens()
        .await?
        .into_iter()
        .find(|t| t.name == "HYPE")
        .map(|t| t.wei_decimals as u32)
        .unwrap_or(8);

    // Build max_gas in wei using discovered decimals.
    let max_gas: u64 = hyperevm::to_wei(args.max, decimals)
        .try_into()
        .map_err(|_| anyhow::anyhow!("--max too large"))?;

    println!(
        "\nBidding on slot {} for IP {} with max {} HYPE ({} decimals)",
        args.slot, args.ip, args.max, decimals
    );

    // 3. Submit the signed bid to /exchange.
    let resp = client
        .gossip_priority_bid(&signer, args.slot, &args.ip, max_gas, nonce, None, None)
        .await?;

    match &resp {
        hypercore::types::Response::Ok(hypercore::types::OkResponse::Default) => {
            println!("Bid submitted successfully.");
        }
        hypercore::types::Response::Err(err) => {
            println!("Bid failed: {}", err);
        }
        _ => {
            println!("Response: {resp:?}");
        }
    }

    // 4. Print updated status so the caller can verify.
    println!("\nRefreshing auction status...");
    let new_status = client.gossip_priority_auction_status().await?;
    print_auction_status(&new_status);

    Ok(())
}
