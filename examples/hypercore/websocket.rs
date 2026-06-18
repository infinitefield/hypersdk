//! Subscribe to real-time market data via WebSocket.
//!
//! This example demonstrates how to use WebSocket subscriptions to receive
//! live price updates for a specific market. It subscribes to all mid prices
//! and filters for KHYPE market updates.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example websocket
//! ```
//!
//! # What it does
//!
//! 1. Connects to Hyperliquid mainnet
//! 2. Queries spot markets to find KHYPE
//! 3. Subscribes to all mid prices via WebSocket
//! 4. Continuously prints KHYPE price updates as they arrive
//!
//! # Output
//!
//! ```text
//! Price of KHYPE/USDC is 1.234
//! Price of KHYPE/USDC is 1.235
//! ...
//! ```
//!
//! # Available Subscriptions
//!
//! - `AllMids`: Mid prices for all markets
//! - `Trades { coin }`: Real-time trades for a specific coin
//! - `L2Book { coin, n_sig_figs, mantissa, fast }`: Order book updates with optional price-level aggregation or fast 5-level mode
//! - `UserEvents { user }`: User-specific events (fills, liquidations)

use anyhow::Context;
use futures::StreamExt;
use hypersdk::hypercore::{
    self as hypercore,
    types::{Incoming, Subscription},
    ws::Event,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = simple_logger::init_with_level(log::Level::Debug);

    let core = hypercore::mainnet();
    let spot = core.spot().await.context("spot")?;

    let khype = spot
        .iter()
        .find(|spot| spot.tokens[0].name == "KHYPE")
        .unwrap();

    let mut ws = core.websocket();
    ws.subscribe(Subscription::AllMids { dex: None });

    while let Some(event) = ws.next().await {
        if let Event::Message(Incoming::AllMids { dex: _, mids }) = event {
            if let Some(price) = mids.get(&khype.name) {
                println!(
                    "Price of {}/{} is {}",
                    khype.tokens[0].name, khype.tokens[1].name, price
                );
            }
        }
    }

    Ok(())
}
