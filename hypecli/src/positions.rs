//! Positions query commands.
//!
//! This module provides commands for querying open perpetual positions.

use std::io::Write;

use clap::{Args, ValueEnum};
use hypersdk::{Address, Decimal, hypercore};
use serde::Serialize;

/// Output format for position data.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable formatted output
    #[default]
    Pretty,
    /// Tab-aligned table output
    Table,
    /// JSON output for programmatic consumption
    Json,
}

/// Serializable position data for JSON output.
#[derive(Serialize)]
struct PositionOutput {
    coin: String,
    size: Decimal,
    side: String,
    entry_price: Option<Decimal>,
    current_value: Decimal,
    unrealized_pnl: Decimal,
    return_on_equity: Decimal,
    liquidation_px: Option<Decimal>,
    margin_used: Decimal,
    leverage: LeverageOutput,
    cum_funding: CumFundingOutput,
}

#[derive(Serialize)]
struct LeverageOutput {
    r#type: String,
    value: u32,
}

#[derive(Serialize)]
struct CumFundingOutput {
    all_time: Decimal,
    since_open: Decimal,
    since_change: Decimal,
}

/// Query open perpetual positions for a user.
///
/// Uses the SDK's clearinghouse state to fetch current positions.
/// Can optionally filter by asset or restrict to a specific DEX.
///
/// # Example
///
/// ```bash
/// hypecli positions 0x1234567890abcdef1234567890abcdef12345678
/// hypecli positions 0x1234... --format table
/// hypecli positions 0x1234... --coin BTC --format json
/// ```
#[derive(Args)]
pub struct PositionsCmd {
    /// User address to query positions for.
    ///
    /// Defaults to your own address if you provide a signer (private key or keystore).
    pub user: Address,

    /// Asset/coin symbol to filter positions (e.g., "BTC", "ETH").
    ///
    /// If omitted, all open positions are shown.
    #[arg(long)]
    pub coin: Option<String>,

    /// HIP-3 DEX name to query positions on.
    ///
    /// Omit to query the default Hyperliquid perp DEX.
    #[arg(long)]
    pub dex: Option<String>,

    /// Output format.
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl PositionsCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = hypercore::HttpClient::new(hypersdk::hypercore::Chain::Mainnet);

        let state = client
            .clearinghouse_state(self.user, self.dex.clone())
            .await?;

        // Filter by coin if specified
        let positions: Vec<_> = state
            .asset_positions
            .into_iter()
            .filter(|p| {
                if let Some(ref coin) = self.coin {
                    p.position.coin.eq_ignore_ascii_case(coin)
                } else {
                    true
                }
            })
            .collect();

        match self.format {
            OutputFormat::Pretty => self.print_pretty(&positions)?,
            OutputFormat::Table => self.print_table(&positions)?,
            OutputFormat::Json => self.print_json(&positions)?,
        }

        Ok(())
    }

    fn print_pretty(
        &self,
        positions: &[hypersdk::hypercore::types::AssetPosition],
    ) -> anyhow::Result<()> {
        if positions.is_empty() {
            let filter = self
                .coin
                .as_ref()
                .map(|c| format!(" for coin '{}'", c))
                .unwrap_or_default();
            println!("No open positions{}.", filter);
            return Ok(());
        }

        println!("Open Positions ({} found):\n", positions.len());

        for pos in positions {
            let p = &pos.position;
            let side = if p.is_long() { "Long" } else { "Short" };

            println!("  {} - {}", p.coin, side);
            println!("  Size:           {}", p.szi);
            println!("  Entry Price:    {}", p.entry_px.map(|e| e.to_string()).unwrap_or_else(|| "N/A".to_string()));
            println!("  Mark Value:     {}", p.position_value);
            println!("  Unrealized PnL: {}", p.unrealized_pnl);
            println!("  Return on Eq:   {}", p.return_on_equity * Decimal::from(100));
            println!("  Margin Used:    {}", p.margin_used);
            if let Some(liq_px) = p.liquidation_px {
                println!("  LiquidationPx:  {}", liq_px);
            }
            let lev_type = match p.leverage.leverage_type {
                hypersdk::hypercore::types::LeverageType::Cross => "cross",
                hypersdk::hypercore::types::LeverageType::Isolated => "isolated",
            };
            println!("  Leverage:       {}x ({})", p.leverage.value, lev_type);
            println!(
                "  Funding:        all_time={} since_open={} since_change={}",
                p.cum_funding.all_time, p.cum_funding.since_open, p.cum_funding.since_change
            );
            println!();
        }

        // Summary
        let total_pnl: Decimal = positions
            .iter()
            .map(|p| &p.position.unrealized_pnl)
            .sum();
        let total_value: Decimal = positions.iter().map(|p| &p.position.position_value).sum();
        println!("{}", "=".repeat(45));
        println!("Total unrealized PnL: {}", total_pnl);
        println!("Total position value: {}", total_value);

        Ok(())
    }

    fn print_table(
        &self,
        positions: &[hypersdk::hypercore::types::AssetPosition],
    ) -> anyhow::Result<()> {
        let mut writer = tabwriter::TabWriter::new(std::io::stdout());

        writeln!(
            writer,
            "coin\tsize\tentry_px\tliquidation_px\tunrealized_pnl\tleverage\tside"
        )?;

        for pos in positions {
            let p = &pos.position;
            let liq_px = p
                .liquidation_px
                .map(|l| l.to_string())
                .unwrap_or_else(|| "-".to_string());
            let entry_px = p
                .entry_px
                .map(|e| e.to_string())
                .unwrap_or_else(|| "-".to_string());
            let lev_val = p.leverage.value;
            let side = if p.is_long() { "long" } else { "short" };

            writeln!(
                writer,
                "{}\t{}\t{}\t{}\t{}\t{}x\t{}",
                p.coin, p.szi, entry_px, liq_px, p.unrealized_pnl, lev_val, side
            )?;
        }

        writer.flush()?;
        Ok(())
    }

    fn print_json(
        &self,
        positions: &[hypersdk::hypercore::types::AssetPosition],
    ) -> anyhow::Result<()> {
        let output: Vec<PositionOutput> = positions
            .iter()
            .map(|p| {
                let lev_val = p.position.leverage.value;
                let lev_type_str = match p.position.leverage.leverage_type {
                    hypersdk::hypercore::types::LeverageType::Cross => "cross",
                    hypersdk::hypercore::types::LeverageType::Isolated => "isolated",
                };
                PositionOutput {
                    coin: p.position.coin.clone(),
                    size: p.position.szi,
                    side: if p.position.is_long() { "long".to_string() } else { "short".to_string() },
                    entry_price: p.position.entry_px,
                    current_value: p.position.position_value,
                    unrealized_pnl: p.position.unrealized_pnl,
                    return_on_equity: p.position.return_on_equity,
                    liquidation_px: p.position.liquidation_px,
                    margin_used: p.position.margin_used,
                    leverage: LeverageOutput {
                        r#type: lev_type_str.to_string(),
                        value: lev_val,
                    },
                    cum_funding: CumFundingOutput {
                        all_time: p.position.cum_funding.all_time,
                        since_open: p.position.cum_funding.since_open,
                        since_change: p.position.cum_funding.since_change,
                    },
                }
            })
            .collect();

        println!("{}", serde_json::to_string_pretty(&output)?);
        Ok(())
    }
}