//! Orders query commands for listing past orders.

use std::io::Write;
use std::net::TcpStream;

use clap::Args;
use serde::Serialize;

use crate::utils::{OutputFormat, QueryArgs};

/// Query past orders from the Hyperliquid REST API.
#[derive(Args)]
pub struct ListOrdersCmd {
    /// Address to query orders for (defaults to current user's perp account)
    #[arg(long)]
    pub user: Option<String>,

    /// Asset filter (e.g., "BTC", "ETH")
    #[arg(long)]
    pub asset: Option<String>,

    /// Common query arguments
    #[command(flatten)]
    pub query: QueryArgs,
}

impl ListOrdersCmd {
    pub fn run(self) -> anyhow::Result<()> {
        let chain = self.query.chain;
        let base_url = match chain.as_str() {
            "mainnet" => "https://api.hyperliquid.xyz",
            "testnet" => "https://api.hyperliquid-testnet.xyz",
            _ => anyhow::bail!("Invalid chain: {}", chain),
        };

        // Build query parameters
        let mut params = vec![
            ("action", "userOrders"),
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
        if let Some(orders) = response.as_array() {
            if orders.is_empty() {
                println!("No orders found.");
                return Ok(());
            }

            println!("Orders ({} found):\n", orders.len());

            for (i, order) in orders.iter().enumerate() {
                if let Some(order_map) = order.as_object() {
                    println!("Order {}:", i + 1);
                    println!(
                        "  Status: {}",
                        order_map.get("status").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Asset: {}",
                        order_map.get("coin").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Side: {}",
                        order_map.get("side").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Size: {}",
                        order_map.get("sz").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  Price: {}",
                        order_map.get("limitPx").and_then(|v| v.as_str()).unwrap_or("unknown")
                    );
                    println!(
                        "  CLOID: {}",
                        order_map.get("cloid").and_then(|v| v.as_str()).unwrap_or("none")
                    );
                    println!();
                }
            }
        }

        Ok(())
    }

    fn print_table(&self, response: &serde_json::Value) -> anyhow::Result<()> {
        let mut writer = tabwriter::TabWriter::new(std::io::stdout());

        writeln!(writer, "status\tcoin\tside\tsize\tprice\tcloid")?;

        if let Some(orders) = response.as_array() {
            for order in orders {
                if let Some(order_map) = order.as_object() {
                    let status = order_map
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let coin = order_map
                        .get("coin")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let side = order_map
                        .get("side")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let sz = order_map.get("sz").and_then(|v| v.as_str()).unwrap_or("-");
                    let limit_px = order_map
                        .get("limitPx")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let cloid = order_map
                        .get("cloid")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");

                    writeln!(writer, "{}\t{}\t{}\t{}\t{}\t{}", status, coin, side, sz, limit_px, cloid)?;
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
