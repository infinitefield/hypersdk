//! Fetch historical funding rates for a perpetual market.
//!
//! This example demonstrates how to query funding rate history for a specific
//! market over a time range. Funding rates are paid/received every 8 hours.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example funding_history
//! ```
//!
//! # Output
//!
//! ```text
//! Funding history for BTC (last 7 days):
//! 2024-01-14 00:00:00  0.0045%  (annualized: 4.93%)
//! 2024-01-14 08:00:00  0.0052%  (annualized: 5.69%)
//! ...
//! ```

use chrono::{DateTime, Utc};
use hypersdk::hypercore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a mainnet client
    let client = hypercore::mainnet();

    // Define time range: last 7 days
    let now = Utc::now().timestamp_millis() as u64;
    let week_ago = now - (7 * 24 * 60 * 60 * 1000);

    // Fetch funding history for BTC
    let coin = "BTC";
    let history = client.funding_history(coin, week_ago, Some(now)).await?;

    println!("Funding history for {} (last 7 days):", coin);
    println!("{:-<60}", "");

    for entry in history.iter().take(20) {
        // Convert timestamp to datetime
        let dt = DateTime::from_timestamp_millis(entry.time as i64)
            .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        println!(
            "{}  {:>8.4}%  (annualized: {:>6.2}%)",
            dt,
            entry.funding_pct(),
            entry.annualized_pct()
        );
    }

    if history.len() > 20 {
        println!("... and {} more entries", history.len() - 20);
    }

    // Calculate average funding rate
    if !history.is_empty() {
        let avg_funding: rust_decimal::Decimal =
            history.iter().map(|e| e.funding_rate).sum::<rust_decimal::Decimal>()
                / rust_decimal::Decimal::from(history.len());
        let avg_annualized = avg_funding * rust_decimal::Decimal::from(1095) * rust_decimal::Decimal::from(100);

        println!("{:-<60}", "");
        println!(
            "Average funding rate: {:.4}% (annualized: {:.2}%)",
            avg_funding * rust_decimal::Decimal::from(100),
            avg_annualized
        );
        println!("Total entries: {}", history.len());
    }

    Ok(())
}
