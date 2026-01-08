//! Subscribe to real-time candlestick data via WebSocket.
//!
//! This example demonstrates how to use WebSocket subscriptions to receive
//! live candlestick (OHLCV) updates for a specific market and interval.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example websocket_candles
//! ```
//!
//! # What it does
//!
//! 1. Connects to Hyperliquid mainnet WebSocket
//! 2. Subscribes to 1-minute BTC candles
//! 3. Continuously prints candle updates as they arrive
//! 4. Displays OHLCV data and calculated metrics
//!
//! # Output
//!
//! ```text
//! BTC 1m candle:
//!   Open:   93250.0
//!   High:   93280.5
//!   Low:    93245.0
//!   Close:  93270.0
//!   Volume: 2.5 BTC
//!   Trades: 42
//!   Change: +20.0 (+0.02%)
//!   Range:  35.5
//! ```
//!
//! # Available Intervals
//!
//! - Minutes: 1m, 3m, 5m, 15m, 30m
//! - Hours: 1h, 2h, 4h, 8h, 12h
//! - Days: 1d, 3d, 1w, 1M

use futures::StreamExt;
use hypersdk::hypercore::{
    self,
    types::{Incoming, Subscription},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger for debug messages
    let _ = simple_logger::init_with_level(log::Level::Info);

    // Create WebSocket connection to mainnet
    let client = hypercore::mainnet();
    let mut ws = client.websocket();

    // Subscribe to 1-minute BTC candles
    ws.subscribe(Subscription::Candle {
        coin: "BTC".to_string(),
        interval: "1m".to_string(),
    });

    log::info!("Subscribed to BTC 1m candles. Waiting for updates...\n");

    // Process incoming candle updates
    while let Some(msg) = ws.next().await {
        match msg {
            Incoming::Candle(candle) => {
                // Calculate some metrics
                let change = candle.close - candle.open;
                let change_pct = if !candle.open.is_zero() {
                    (change / candle.open) * rust_decimal::Decimal::ONE_HUNDRED
                } else {
                    rust_decimal::Decimal::ZERO
                };
                let range = candle.high - candle.low;

                // Print formatted candle data
                println!("{} {} candle:", candle.coin, candle.interval);
                println!("  Open:   {}", candle.open);
                println!("  High:   {}", candle.high);
                println!("  Low:    {}", candle.low);
                println!("  Close:  {}", candle.close);
                println!("  Volume: {} {}", candle.volume, candle.coin);
                println!("  Trades: {}", candle.num_trades);
                println!(
                    "  Change: {} ({:+.2}%)",
                    if change.is_sign_positive() {
                        format!("+{}", change)
                    } else {
                        change.to_string()
                    },
                    change_pct
                );
                println!("  Range:  {}", range);
                println!("  Time:   {} - {}\n", candle.open_time, candle.close_time);
            }
            Incoming::SubscriptionResponse(_) => {
                println!("Subscription confirmed");
            }
            Incoming::Ping => {
                // Server sent ping, connection is alive
            }
            _ => {}
        }
    }

    Ok(())
}
