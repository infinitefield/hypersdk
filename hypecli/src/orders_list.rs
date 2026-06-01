//! Order and fill query commands.
//!
//! This module provides commands for querying historical orders and fills
//! from the Hyperliquid perpetual exchange.

use std::io::Write;

use clap::{Args, Subcommand, ValueEnum};
use hypersdk::{Address, Decimal, hypercore};
use hypercore::types::OrderUpdate;
use serde::Serialize;

/// Output format for order/fill data.
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

/// Serializable fill data for JSON output.
#[derive(Serialize)]
struct FillOutput {
    coin: String,
    side: String,
    px: Decimal,
    sz: Decimal,
    notional: Decimal,
    fee: Decimal,
    realized_pnl: Decimal,
    start_position: Decimal,
    direction: String,
    crossed: bool,
    oid: u64,
    time_ms: u64,
    hash: String,
}

/// Serializable basic order data for JSON output.
#[derive(Serialize)]
struct OrderOutput {
    timestamp: u64,
    coin: String,
    side: String,
    limit_px: Decimal,
    sz: Decimal,
    oid: u64,
    orig_sz: Decimal,
    cloid: Option<String>,
    order_type: String,
    tif: Option<String>,
    reduce_only: bool,
}

/// Query commands.
#[derive(Subcommand)]
pub enum OrdersCmd {
    /// Query historical (filled/canceled) orders.
    List(ListOrdersCmd),
    /// Query your trade fills.
    Fills(FillsCmd),
}

impl OrdersCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Self::List(cmd) => cmd.run().await,
            Self::Fills(cmd) => cmd.run().await,
        }
    }
}

// ---------------------------------------------------------------------------
// ListOrdersCmd
// ---------------------------------------------------------------------------

/// Query historical (filled and canceled) orders.
///
/// # Example
///
/// ```bash
/// hypecli orders list 0x1234567890abcdef1234567890abcdef12345678
/// hypecli orders list 0x1234... --coin BTC --format json
/// ```
#[derive(Args)]
pub struct ListOrdersCmd {
    /// User address to query orders for.
    pub user: Address,

    /// Asset/coin symbol to filter (e.g., "BTC", "ETH").
    #[arg(long)]
    pub coin: Option<String>,

    /// Output format.
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl ListOrdersCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = hypercore::HttpClient::new(hypersdk::hypercore::Chain::Mainnet);

        let orders = client.historical_orders(self.user).await?;

        // Filter by coin if specified
        let orders: Vec<_> = orders
            .into_iter()
            .filter(|o| {
                if let Some(ref coin) = self.coin {
                    o.order.coin.eq_ignore_ascii_case(coin)
                } else {
                    true
                }
            })
            .collect();

        match self.format {
            OutputFormat::Pretty => self.print_pretty(&orders)?,
            OutputFormat::Table => self.print_table(&orders)?,
            OutputFormat::Json => self.print_json(&orders)?,
        }

        Ok(())
    }

    fn print_pretty(
        &self,
        updates: &[OrderUpdate<hypersdk::hypercore::types::BasicOrder>],
    ) -> anyhow::Result<()> {
        if updates.is_empty() {
            let filter = self.coin.as_ref().map(|c| format!(" for '{}'", c)).unwrap_or_default();
            println!("No orders found{}.", filter);
            return Ok(());
        }

        println!("Historical Orders ({} found):\n", updates.len());

        for u in updates {
            let order = &u.order;
            let ts = chrono::DateTime::from_timestamp_millis(order.timestamp as i64)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| format!("{}ms", order.timestamp));
            println!("  {} | {:?} | {} {} @ {}", ts, order.order_type, order.side, order.sz, order.limit_px);
            println!("    Coin:      {}", order.coin);
            println!("    Status:    {:?}", u.status);
            println!("    OID:       {}", order.oid);
            if let Some(ref cloid) = order.cloid {
                println!("    CLOID:     {}", cloid);
            }
            if order.reduce_only {
                println!("    reduce-only");
            }
            if let Some(tif) = order.tif {
                println!("    TIF:       {:?}", tif);
            }
            println!();
        }

        Ok(())
    }

    fn print_table(
        &self,
        updates: &[OrderUpdate<hypersdk::hypercore::types::BasicOrder>],
    ) -> anyhow::Result<()> {
        let mut writer = tabwriter::TabWriter::new(std::io::stdout());
        writeln!(writer, "timestamp\tcoin\tside\tlimit_px\tsz\torig_sz\toid\tcloid\tstatus")?;

        for u in updates {
            let order = &u.order;
            writeln!(
                writer,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:?}",
                order.timestamp,
                order.coin,
                order.side,
                order.limit_px,
                order.sz,
                order.orig_sz,
                order.oid,
                order.cloid.as_ref().map(|c| c.to_string()).unwrap_or_else(|| "-".to_string()),
                u.status
            )?;
        }
        writer.flush()?;
        Ok(())
    }

    fn print_json(
        &self,
        updates: &[OrderUpdate<hypersdk::hypercore::types::BasicOrder>],
    ) -> anyhow::Result<()> {
        let output: Vec<OrderOutput> = updates
            .iter()
            .map(|u| {
                let o = &u.order;
                OrderOutput {
                    timestamp: o.timestamp,
                    coin: o.coin.clone(),
                    side: o.side.to_string(),
                    limit_px: o.limit_px,
                    sz: o.sz,
                    oid: o.oid,
                    orig_sz: o.orig_sz,
                    cloid: o.cloid.as_ref().map(|c| c.to_string()),
                    order_type: format!("{:?}", o.order_type),
                    tif: o.tif.map(|t| format!("{:?}", t)),
                    reduce_only: o.reduce_only,
                }
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// FillsCmd
// ---------------------------------------------------------------------------

/// Query your trade fills.
///
/// Shows each individual market fill: price, size, fees, realized PnL, side,
/// and whether you were the taker (crossed the spread).
///
/// # Example
///
/// ```bash
/// hypecli orders fills 0x1234567890abcdef1234567890abcdef12345678
/// hypecli orders fills 0x1234... --coin BTC --format table
/// ```
#[derive(Args)]
pub struct FillsCmd {
    /// User address to query fills for.
    pub user: Address,

    /// Asset/coin symbol to filter (e.g., "BTC", "ETH").
    #[arg(long)]
    pub coin: Option<String>,

    /// Output format.
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl FillsCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = hypercore::HttpClient::new(hypersdk::hypercore::Chain::Mainnet);

        let fills = client.user_fills(self.user).await?;

        // Filter by coin if specified
        let fills: Vec<_> = fills
            .into_iter()
            .filter(|f| {
                if let Some(ref coin) = self.coin {
                    f.coin.eq_ignore_ascii_case(coin)
                } else {
                    true
                }
            })
            .collect();

        match self.format {
            OutputFormat::Pretty => self.print_pretty(&fills)?,
            OutputFormat::Table => self.print_table(&fills)?,
            OutputFormat::Json => self.print_json(&fills)?,
        }

        Ok(())
    }

    fn print_pretty(
        &self,
        fills: &[hypersdk::hypercore::types::Fill],
    ) -> anyhow::Result<()> {
        if fills.is_empty() {
            let filter = self.coin.as_ref().map(|c| format!(" for '{}'", c)).unwrap_or_default();
            println!("No fills found{}.", filter);
            return Ok(());
        }

        // Summary stats
        let total_fee: Decimal = fills.iter().map(|f| &f.fee).sum();
        let total_notional: Decimal = fills.iter().map(|f| f.notional()).sum();
        let total_rpnl: Decimal = fills.iter().map(|f| &f.closed_pnl).sum();

        println!(
            "Fills ({} found) | Total notional: {} | Fees: {} | Realized PnL: {}",
            fills.len(), total_notional, total_fee, total_rpnl
        );
        println!();

        for fill in fills {
            let ts = chrono::DateTime::from_timestamp_millis(fill.time as i64)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| format!("{}ms", fill.time));
            let role = if fill.crossed { "Taker" } else { "Maker" };

            println!("  {} [{}] | {} {} {} @ {} (notional: {})",
                ts, role,
                fill.dir, fill.side, fill.sz, fill.px, fill.notional()
            );
            println!("    Fee:          {}", fill.fee);
            if fill.closed_pnl != Decimal::ZERO {
                println!("    Closed PnL:   {}", fill.closed_pnl);
            }
            if let Some(ref liq) = fill.liquidation {
                println!("    Liquidation:  {:?}", liq);
            }
            println!("    OID:          {} | Hash: {}", fill.oid, &fill.hash[..8.min(fill.hash.len())]);
            println!();
        }

        Ok(())
    }

    fn print_table(
        &self,
        fills: &[hypersdk::hypercore::types::Fill],
    ) -> anyhow::Result<()> {
        let mut writer = tabwriter::TabWriter::new(std::io::stdout());
        writeln!(writer, "time\tcoin\tside\tsx\tpx\tnotional\tfee\trPnL\tcrossed\toid")?;

        for fill in fills {
            writeln!(
                writer,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                fill.time,
                fill.coin,
                fill.side,
                fill.sz,
                fill.px,
                fill.notional(),
                fill.fee,
                fill.closed_pnl,
                if fill.crossed { "taker" } else { "maker" },
                fill.oid
            )?;
        }
        writer.flush()?;
        Ok(())
    }

    fn print_json(
        &self,
        fills: &[hypersdk::hypercore::types::Fill],
    ) -> anyhow::Result<()> {
        let output: Vec<FillOutput> = fills
            .iter()
            .map(|f| FillOutput {
                coin: f.coin.clone(),
                side: f.side.to_string(),
                px: f.px,
                sz: f.sz,
                notional: f.notional(),
                fee: f.fee,
                realized_pnl: f.closed_pnl,
                start_position: f.start_position,
                direction: f.dir.to_string(),
                crossed: f.crossed,
                oid: f.oid,
                time_ms: f.time,
                hash: f.hash.clone(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
        Ok(())
    }
}
