//! List all perpetual markets grouped by their DEX.
//!
//! Queries the perp dexes endpoint and prints each market's name, index, collateral,
//! growth mode, and aligned quote token along with deployer fee scale information.

use hypersdk::hypercore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = hypercore::mainnet();

    let dexes = client.perp_dexes().await?;
    for dex in dexes {
        println!("\n\nmarkets for {dex}");

        let markets = client.perps_from(dex).await?;
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
    }

    Ok(())
}
