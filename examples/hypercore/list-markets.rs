//! List all available spot markets on Hyperliquid.
//!
//! This example demonstrates how to query all spot trading pairs and display
//! their basic information including market name and token pairs.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example list-markets
//! ```
//!
//! # Output
//!
//! ```text
//! PURR-SPOT    PURR/USDC
//! HYPE-SPOT    HYPE/USDC
//! ...
//! ```

use hypersdk::hypercore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a mainnet client
    let client = hypercore::mainnet();

    // Fetch all spot markets
    let markets = client.spot().await?;

    // Display market information
    for market in markets {
        println!(
            "{}\t{}/{}",
            market.name, market.tokens[0].name, market.tokens[1].name
        );
    }

    // Fetch all perps markets
    let markets = client.perps().await?;

    // Display market information
    for market in markets {
        println!(
            "{}\t{}\t{}\t{}\t{:?}\t{}\t{}",
            market.name,
            market.index,
            market.name,
            market.collateral,
            market.deployer_fee_scale,
            market.growth_mode,
            market.aligned_quote_token
        );
    }

    Ok(())
}
