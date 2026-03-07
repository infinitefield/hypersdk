//! Positions query commands for querying current open positions.

use clap::Args;
use serde::Serialize;

use crate::utils::{OutputFormat, QueryArgs};

/// Query current open positions from the Hyperliquid REST API.
#[derive(Args)]
pub struct PositionsCmd {
    /// Address to query positions for (defaults to current user's perp account)
    #[arg(long)]
    pub user: Option<String>,

    /// Asset filter (e.g., "BTC", "ETH")
    #[arg(long)]
    pub asset: Option<String>,

    /// Common query arguments
    #[command(flatten)]
    pub query: QueryArgs,
}

impl PositionsCmd {
    pub fn run(self) -> anyhow::Result<()> {
        let chain = self.query.chain;
        let base_url = match chain.as_str() {
            "mainnet" => "https://api.hyperliquid.xyz",
            "testnet" => "https://api.hyperliquid-testnet.xyz",
            _ => anyhow::bail!("Invalid chain: {}", chain),
        };

        // Build query parameters
        let mut params = vec![
            ("action", "positions"),
            ("user", self.user.as_deref().unwrap_or("0x0000000000000000000000000000000000000000")),
        ];

        if let Some(asset) = &self.asset {
            params.push(("coin", asset));
        }

        let query_string = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        let url = format!("{}?{}", base_url, query_string);
        
        let response = reqwest::blocking::get(&url)?
            .json::<serde_json::Value>()?;

        match self.query.format {
            OutputFormat::Pretty => self.print_pretty(&response)?,
            OutputFormat::Table => self.print_table(&response)?,
            OutputFormat::Json => self.print_json(&response)?,
        }

        Ok(())
    }

    fn print_pretty(&self, response: &serde_json::Value) -> anyhow::Result<()> {
        if let Some(positions) = response.as_array() {
            if positions.is_empty() {
                println!("No open positions found.");
                return Ok(());
            }

            println!("Open Positions ({} found):\n", positions.len());

            for (i, pos) in positions.iter().enumerate() {
                if let Some(pos_map) = pos.as_object() {
                    println!("Position {}:", i + 1);
                    println!(
                        "  Asset: {}",
                        pos_map.get("coin").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Size: {}",
                        pos_map.get("szi").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Entry Price: {}",
                        pos_map.get("entryPx").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Unrealized PnL: {}",
                        pos_map.get("unrealizedPnl").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Leverage: {}",
                        pos_map.get("leverage").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Side: {}",
                        pos_map.get("side").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!();
                }
            }
        }

        Ok(())
    }

    fn print_table(&self, response: &serde_json::Value) -> anyhow::Result<()> {
        let mut writer = tabwriter::TabWriter::new(std::io::stdout());

        writeln!(writer, "coin\tsize\tentry_price\tunrealized_pnl\tleverage\tside")?;

        if let Some(positions) = response.as_array() {
            for pos in positions {
                if let Some(pos_map) = pos.as_object() {
                    let coin = pos_map
                        .get("coin")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let szi = pos_map.get("szi").and_then(|v| v.as_str()).unwrap_or("-");
                    let entry_px = pos_map
                        .get("entryPx")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let unrealized_pnl = pos_map
                        .get("unrealizedPnl")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let leverage = pos_map
                        .get("leverage")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let side = pos_map
                        .get("side")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");

                    writeln!(writer, "{}\t{}\t{}\t{}\t{}\t{}", coin, szi, entry_px, unrealized_pnl, leverage, side)?;
                }
            }
        }

        writer.flush()?;
        Ok(())
    }

    fn print_json(&self, response: &serde_json::Value) -> anyhow::Result<()> {
        println!("{}", serde_json::to_string_pretty(response)?);
        Ok(())
    }
}
