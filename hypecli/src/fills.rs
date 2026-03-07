//! Fill history query commands for querying past fill executions.

use std::io::Write;

use clap::Args;
use serde::Serialize;

use crate::utils::{OutputFormat, QueryArgs};

/// Query fill history from the Hyperliquid REST API.
#[derive(Args)]
pub struct FillCmd {
    /// Address to query fills for (defaults to current user's perp account)
    #[arg(long)]
    pub user: Option<String>,

    /// Asset filter (e.g., "BTC", "ETH")
    #[arg(long)]
    pub asset: Option<String>,

    /// Common query arguments
    #[command(flatten)]
    pub query: QueryArgs,
}

impl FillCmd {
    pub fn run(self) -> anyhow::Result<()> {
        let chain = self.query.chain;
        let base_url = match chain.as_str() {
            "mainnet" => "https://api.hyperliquid.xyz",
            "testnet" => "https://api.hyperliquid-testnet.xyz",
            _ => anyhow::bail!("Invalid chain: {}", chain),
        };

        // Build query parameters
        let mut params = vec![
            ("action", "fills"),
            ("coin", self.asset.as_deref().unwrap_or("ALL")),
            ("limit", &self.query.limit.to_string()),
        ];

        if let Some(user) = &self.user {
            params.push(("user", user));
        }

        let query_string = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        let url = format!("{}?{}", base_url, query_string);
        
        // Execute query using Hyperliquid REST API
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
        if let Some(fills) = response.as_array() {
            if fills.is_empty() {
                println!("No fills found.");
                return Ok(());
            }

            println!("Fill History ({} found):\n", fills.len());

            for (i, fill) in fills.iter().enumerate() {
                if let Some(fill_map) = fill.as_object() {
                    println!("Fill {}:", i + 1);
                    println!(
                        "  Time: {}",
                        fill_map.get("time").and_then(|v| v.as_u64()).map_or("unknown".to_string(), |t| t.to_string())
                    );
                    println!(
                        "  Status: {}",
                        fill_map.get("status").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Asset: {}",
                        fill_map.get("coin").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Side: {}",
                        fill_map.get("side").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Size: {}",
                        fill_map.get("sz").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Price: {}",
                        fill_map.get("px").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Is Maker: {}",
                        fill_map.get("isMaker").and_then(|v| v.as_bool()).map_or(false, |b| b)
                    );
                    println!(
                        "  CLOID: {}",
                        fill_map.get("cloid").and_then(|v| v.as_str()).unwrap_or("none")
                    );
                    println!();
                }
            }
        }

        Ok(())
    }

    fn print_table(&self, response: &serde_json::Value) -> anyhow::Result<()> {
        let mut writer = tabwriter::TabWriter::new(std::io::stdout());

        writeln!(writer, "time\tstatus\tcoin\tside\tsize\tprice\tis_maker\tcloid")?;

        if let Some(fills) = response.as_array() {
            for fill in fills {
                if let Some(fill_map) = fill.as_object() {
                    let time = fill_map
                        .get("time")
                        .and_then(|v| v.as_u64())
                        .map_or("-".to_string(), |t| t.to_string());
                    let status = fill_map
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let coin = fill_map
                        .get("coin")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let side = fill_map
                        .get("side")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let sz = fill_map.get("sz").and_then(|v| v.as_str()).unwrap_or("-");
                    let px = fill_map.get("px").and_then(|v| v.as_str()).unwrap_or("-");
                    let is_maker = fill_map
                        .get("isMaker")
                        .and_then(|v| v.as_bool())
                        .map_or(false, |b| b.to_string());
                    let cloid = fill_map
                        .get("cloid")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");

                    writeln!(writer, "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", time, status, coin, side, sz, px, is_maker, cloid)?;
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
