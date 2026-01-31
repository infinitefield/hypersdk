//! Order management commands for placing and canceling orders.
//!
//! This module provides CLI commands for:
//! - Placing limit orders
//! - Placing market orders
//! - Canceling orders (by OID or CLOID)
//!
//! ## Asset Name Formats
//!
//! Assets are specified by name using the following conventions:
//! - `BTC` - BTC perpetual on Hyperliquid DEX
//! - `PURR/USDC` - PURR spot market
//! - `xyz:BTC` - BTC perpetual on the "xyz" HIP3 DEX

use alloy::primitives::B128;
use clap::{Args, Subcommand, ValueEnum};
use hypersdk::hypercore::{
    BatchCancel, BatchCancelCloid, BatchOrder, Cancel, CancelByCloid, Cloid, HttpClient,
    OrderGrouping, OrderRequest, OrderTypePlacement, PerpMarket, SpotMarket, TimeInForce,
};
use rust_decimal::Decimal;

use crate::SignerArgs;
use crate::utils::find_signer_sync;

/// Order management commands.
#[derive(Subcommand)]
pub enum OrderCmd {
    /// Place a limit order
    Limit(LimitOrderCmd),
    /// Place a market order
    Market(MarketOrderCmd),
    /// Cancel an order by OID or CLOID
    Cancel(CancelOrderCmd),
}

impl OrderCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Limit(cmd) => cmd.run().await,
            Self::Market(cmd) => cmd.run().await,
            Self::Cancel(cmd) => cmd.run().await,
        }
    }
}

/// Order side (buy or sell).
#[derive(Clone, Copy, ValueEnum)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    fn is_buy(&self) -> bool {
        matches!(self, Side::Buy)
    }
}

/// Time-in-force option for limit orders.
#[derive(Clone, Copy, ValueEnum, Default)]
pub enum Tif {
    /// Good Till Cancel - standard order that remains until filled or canceled
    #[default]
    Gtc,
    /// Add Liquidity Only - maker-only order, rejected if it would take
    Alo,
    /// Immediate or Cancel - fill immediately or cancel unfilled portion
    Ioc,
}

impl From<Tif> for TimeInForce {
    fn from(tif: Tif) -> Self {
        match tif {
            Tif::Gtc => TimeInForce::Gtc,
            Tif::Alo => TimeInForce::Alo,
            Tif::Ioc => TimeInForce::Ioc,
        }
    }
}

/// Place a limit order.
#[derive(Args, derive_more::Deref)]
pub struct LimitOrderCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Asset name. Formats:
    /// - "BTC" for BTC perpetual
    /// - "PURR/USDC" for PURR spot market
    /// - "xyz:BTC" for BTC perpetual on xyz HIP3 DEX
    #[arg(long)]
    pub asset: String,

    /// Order side (buy or sell)
    #[arg(long)]
    pub side: Side,

    /// Limit price
    #[arg(long)]
    pub price: Decimal,

    /// Order size
    #[arg(long)]
    pub size: Decimal,

    /// Reduce-only order (can only reduce existing position)
    #[arg(long, default_value = "false")]
    pub reduce_only: bool,

    /// Time-in-force (gtc, alo, ioc)
    #[arg(long, default_value = "gtc")]
    pub tif: Tif,

    /// Optional client order ID (hex string, 16 bytes)
    #[arg(long)]
    pub cloid: Option<String>,
}

impl LimitOrderCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let signer = find_signer_sync(&self.signer)?;

        let asset_index = resolve_asset(&client, &self.asset).await?;

        let cloid = parse_cloid(self.cloid.as_deref())?;

        println!(
            "Placing limit order for {} (index {}) with signer {}",
            self.asset,
            asset_index,
            signer.address()
        );
        println!("CLOID: 0x{}", hex::encode(cloid.as_slice()));

        let order = OrderRequest {
            asset: asset_index,
            is_buy: self.side.is_buy(),
            limit_px: self.price,
            sz: self.size,
            reduce_only: self.reduce_only,
            order_type: OrderTypePlacement::Limit {
                tif: self.tif.into(),
            },
            cloid,
        };

        let batch = BatchOrder {
            orders: vec![order],
            grouping: OrderGrouping::Na,
        };

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        let result = client.place(&signer, batch, nonce, None, None).await;

        match result {
            Ok(statuses) => {
                println!("Order placed successfully:");
                for (i, status) in statuses.iter().enumerate() {
                    println!("  Order {}: {:?}", i, status);
                }
            }
            Err(err) => {
                anyhow::bail!("Order failed: {}", err.message());
            }
        }

        Ok(())
    }
}

/// Place a market order.
#[derive(Args, derive_more::Deref)]
pub struct MarketOrderCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Asset name. Formats:
    /// - "BTC" for BTC perpetual
    /// - "PURR/USDC" for PURR spot market
    /// - "xyz:BTC" for BTC perpetual on xyz HIP3 DEX
    #[arg(long)]
    pub asset: String,

    /// Order side (buy or sell)
    #[arg(long)]
    pub side: Side,

    /// Order size
    #[arg(long)]
    pub size: Decimal,

    /// Slippage price (worst acceptable price for the market order)
    #[arg(long)]
    pub slippage_price: Decimal,

    /// Reduce-only order (can only reduce existing position)
    #[arg(long, default_value = "false")]
    pub reduce_only: bool,

    /// Optional client order ID (hex string, 16 bytes)
    #[arg(long)]
    pub cloid: Option<String>,
}

impl MarketOrderCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let signer = find_signer_sync(&self.signer)?;

        let asset_index = resolve_asset(&client, &self.asset).await?;

        let cloid = parse_cloid(self.cloid.as_deref())?;

        println!(
            "Placing market order for {} (index {}) with signer {}",
            self.asset,
            asset_index,
            signer.address()
        );
        println!("CLOID: 0x{}", hex::encode(cloid.as_slice()));

        // Market orders use FrontendMarket TIF with a slippage price
        let order = OrderRequest {
            asset: asset_index,
            is_buy: self.side.is_buy(),
            limit_px: self.slippage_price,
            sz: self.size,
            reduce_only: self.reduce_only,
            order_type: OrderTypePlacement::Limit {
                tif: TimeInForce::FrontendMarket,
            },
            cloid,
        };

        let batch = BatchOrder {
            orders: vec![order],
            grouping: OrderGrouping::Na,
        };

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        let result = client.place(&signer, batch, nonce, None, None).await;

        match result {
            Ok(statuses) => {
                println!("Market order placed successfully:");
                for (i, status) in statuses.iter().enumerate() {
                    println!("  Order {}: {:?}", i, status);
                }
            }
            Err(err) => {
                anyhow::bail!("Market order failed: {}", err.message());
            }
        }

        Ok(())
    }
}

/// Cancel an order by OID or CLOID.
///
/// Specify either `--oid` for exchange-assigned order ID or `--cloid` for client-assigned order ID.
#[derive(Args, derive_more::Deref)]
pub struct CancelOrderCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Asset name. Formats:
    /// - "BTC" for BTC perpetual
    /// - "PURR/USDC" for PURR spot market
    /// - "xyz:BTC" for BTC perpetual on xyz HIP3 DEX
    #[arg(long)]
    pub asset: String,

    /// Exchange-assigned order ID to cancel
    #[arg(long)]
    pub oid: Option<u64>,

    /// Client-assigned order ID to cancel (hex string, 16 bytes)
    #[arg(long)]
    pub cloid: Option<String>,
}

impl CancelOrderCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        // Validate that exactly one of oid or cloid is provided
        match (&self.oid, &self.cloid) {
            (None, None) => anyhow::bail!("Must specify either --oid or --cloid"),
            (Some(_), Some(_)) => anyhow::bail!("Cannot specify both --oid and --cloid"),
            _ => {}
        }

        let client = HttpClient::new(self.chain);
        let signer = find_signer_sync(&self.signer)?;

        let asset_index = resolve_asset(&client, &self.asset).await?;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64;

        if let Some(cloid) = &self.cloid {
            // Cancel by CLOID
            let cloid_bytes = parse_cloid_required(cloid)?;

            println!(
                "Canceling order by CLOID for {} (index {}) with signer {}",
                self.asset,
                asset_index,
                signer.address()
            );
            println!("CLOID: {}", cloid);

            let batch = BatchCancelCloid {
                cancels: vec![CancelByCloid {
                    asset: asset_index as u32,
                    cloid: cloid_bytes,
                }],
            };

            let result = client
                .cancel_by_cloid(&signer, batch, nonce, None, None)
                .await;

            match result {
                Ok(statuses) => {
                    println!("Order canceled successfully:");
                    for (i, status) in statuses.iter().enumerate() {
                        println!("  Cancel {}: {:?}", i, status);
                    }
                }
                Err(err) => {
                    anyhow::bail!("Cancel failed: {}", err.message());
                }
            }
        } else if let Some(oid) = self.oid {
            // Cancel by OID
            println!(
                "Canceling order by OID for {} (index {}) with signer {}",
                self.asset,
                asset_index,
                signer.address()
            );
            println!("OID: {}", oid);

            let batch = BatchCancel {
                cancels: vec![Cancel {
                    asset: asset_index,
                    oid,
                }],
            };

            let result = client.cancel(&signer, batch, nonce, None, None).await;

            match result {
                Ok(statuses) => {
                    println!("Order canceled successfully:");
                    for (i, status) in statuses.iter().enumerate() {
                        println!("  Cancel {}: {:?}", i, status);
                    }
                }
                Err(err) => {
                    anyhow::bail!("Cancel failed: {}", err.message());
                }
            }
        }

        Ok(())
    }
}

/// Parsed asset specification.
enum AssetSpec<'a> {
    /// Perpetual on Hyperliquid DEX (e.g., "BTC")
    Perp(&'a str),
    /// Spot market (e.g., "PURR/USDC")
    Spot(&'a str, &'a str),
    /// Perpetual on HIP3 DEX (e.g., "xyz:BTC")
    Hip3Perp(&'a str, &'a str),
}

/// Parse an asset name string into an AssetSpec.
///
/// Formats:
/// - "BTC" → Perp("BTC")
/// - "PURR/USDC" → Spot("PURR", "USDC")
/// - "xyz:BTC" → Hip3Perp("xyz", "BTC")
fn parse_asset_spec(asset: &str) -> anyhow::Result<AssetSpec<'_>> {
    if let Some((base, quote)) = asset.split_once('/') {
        // Spot market: BASE/QUOTE
        Ok(AssetSpec::Spot(base, quote))
    } else if let Some((dex, symbol)) = asset.split_once(':') {
        // HIP3 DEX: dex:SYMBOL
        Ok(AssetSpec::Hip3Perp(dex, symbol))
    } else {
        // Default: Hyperliquid perp
        Ok(AssetSpec::Perp(asset))
    }
}

/// Resolve an asset name to its index.
///
/// This queries the appropriate market data based on the asset format
/// and returns the asset index for use in API calls.
async fn resolve_asset(client: &HttpClient, asset: &str) -> anyhow::Result<usize> {
    let spec = parse_asset_spec(asset)?;

    match spec {
        AssetSpec::Perp(symbol) => {
            let perps = client.perps().await?;
            find_perp_index(&perps, symbol, None)
        }
        AssetSpec::Spot(base, quote) => {
            let spots = client.spot().await?;
            find_spot_index(&spots, base, quote)
        }
        AssetSpec::Hip3Perp(dex_name, symbol) => {
            // First get the DEX
            let dexs = client.perp_dexs().await?;
            let dex = dexs
                .iter()
                .find(|d| d.name().eq_ignore_ascii_case(dex_name))
                .ok_or_else(|| anyhow::anyhow!("HIP3 DEX '{}' not found", dex_name))?;

            // Then get perps from that DEX
            let perps = client.perps_from(dex.clone()).await?;
            find_perp_index(&perps, symbol, Some(dex_name))
        }
    }
}

/// Find a perpetual market index by symbol.
fn find_perp_index(
    perps: &[PerpMarket],
    symbol: &str,
    dex_name: Option<&str>,
) -> anyhow::Result<usize> {
    perps
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(symbol))
        .map(|p| p.index)
        .ok_or_else(|| {
            let dex_desc = dex_name
                .map(|d| format!(" on {} DEX", d))
                .unwrap_or_default();
            anyhow::anyhow!(
                "Perpetual market '{}'{} not found. Use 'hypecli perps' to list available markets.",
                symbol,
                dex_desc
            )
        })
}

/// Find a spot market index by base and quote symbols.
fn find_spot_index(spots: &[SpotMarket], base: &str, quote: &str) -> anyhow::Result<usize> {
    spots
        .iter()
        .find(|s| {
            s.base().name.eq_ignore_ascii_case(base) && s.quote().name.eq_ignore_ascii_case(quote)
        })
        .map(|s| s.index)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Spot market '{}/{}' not found. Use 'hypecli spot' to list available markets.",
                base,
                quote
            )
        })
}

/// Parse an optional CLOID string into a B128.
/// If None is provided, generates a random CLOID.
fn parse_cloid(cloid: Option<&str>) -> anyhow::Result<Cloid> {
    match cloid {
        Some(s) => parse_cloid_required(s),
        None => Ok(B128::random()),
    }
}

/// Parse a required CLOID string into a B128.
fn parse_cloid_required(cloid: &str) -> anyhow::Result<B128> {
    let cloid = cloid.strip_prefix("0x").unwrap_or(cloid);
    let bytes = hex::decode(cloid)?;
    if bytes.len() != 16 {
        anyhow::bail!("CLOID must be exactly 16 bytes (32 hex characters)");
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(&bytes);
    Ok(B128::from(arr))
}
