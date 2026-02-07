//! WebSocket subscription commands for real-time market data.
//!
//! This module provides commands for subscribing to various WebSocket feeds
//! including trades, orderbook updates, best bid/offer, candles, and user events.
//!
//! ## Asset Name Formats
//!
//! Assets are specified using the same unified format as order commands:
//! - `BTC` - BTC perpetual on Hyperliquid DEX
//! - `PURR/USDC` - PURR spot market
//! - `xyz:BTC` - BTC perpetual on the "xyz" HIP3 DEX

use std::io::{Write, stdout};

use alloy::primitives::Address;
use clap::{Args, Subcommand, ValueEnum};
use futures::StreamExt;
use hypersdk::hypercore::{
    self, Chain, HttpClient,
    types::{Incoming, Subscription},
    ws::Event,
};
use rust_decimal::Decimal;

use crate::utils::resolve_asset_for_subscription;

/// Output format for subscription data.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable formatted output
    #[default]
    Pretty,
    /// JSON output (one message per line)
    Json,
}

/// Subscribe to real-time WebSocket data feeds.
#[derive(Subcommand)]
pub enum SubscribeCmd {
    /// Subscribe to real-time trades for a coin
    Trades(TradesCmd),
    /// Subscribe to best bid/offer updates
    Bbo(BboCmd),
    /// Subscribe to order book updates
    Orderbook(OrderbookCmd),
    /// Subscribe to candlestick (OHLCV) data
    Candles(CandlesCmd),
    /// Subscribe to mid prices for all markets
    AllMids(AllMidsCmd),
    /// Subscribe to order updates for a user
    OrderUpdates(OrderUpdatesCmd),
    /// Subscribe to fill events for a user
    Fills(FillsCmd),
}

impl SubscribeCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Trades(cmd) => cmd.run().await,
            Self::Bbo(cmd) => cmd.run().await,
            Self::Orderbook(cmd) => cmd.run().await,
            Self::Candles(cmd) => cmd.run().await,
            Self::AllMids(cmd) => cmd.run().await,
            Self::OrderUpdates(cmd) => cmd.run().await,
            Self::Fills(cmd) => cmd.run().await,
        }
    }
}

/// Subscribe to real-time trades for an asset.
///
/// # Example
///
/// ```bash
/// hypecli subscribe trades --asset BTC
/// hypecli subscribe trades --asset PURR/USDC
/// hypecli subscribe trades --asset xyz:BTC --format json
/// ```
#[derive(Args)]
pub struct TradesCmd {
    /// Asset name. Formats:
    /// - "BTC" for BTC perpetual
    /// - "PURR/USDC" for PURR spot market
    /// - "xyz:BTC" for BTC perpetual on xyz HIP3 DEX
    #[arg(long)]
    pub asset: String,
    /// Target chain
    #[arg(long, default_value = "Mainnet")]
    pub chain: Chain,
    /// Output format
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl TradesCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let resolved = resolve_asset_for_subscription(&client, &self.asset).await?;

        let core = match self.chain {
            Chain::Mainnet => hypercore::mainnet(),
            Chain::Testnet => hypercore::testnet(),
        };

        let mut ws = core.websocket();
        ws.subscribe(Subscription::Trades {
            coin: resolved.coin.clone(),
        });

        eprintln!("Subscribing to {} trades...", self.asset);

        while let Some(event) = ws.next().await {
            match event {
                Event::Connected => eprintln!("Connected"),
                Event::Disconnected => eprintln!("Disconnected, reconnecting..."),
                Event::Message(msg) => match msg {
                    Incoming::Trades(trades) => {
                        for trade in trades {
                            match self.format {
                                OutputFormat::Pretty => {
                                    println!(
                                        "{} {} {} @ {} (notional: {})",
                                        trade.coin,
                                        trade.side,
                                        trade.sz,
                                        trade.px,
                                        trade.notional()
                                    );
                                }
                                OutputFormat::Json => {
                                    println!("{}", serde_json::to_string(&trade)?);
                                }
                            }
                        }
                    }
                    Incoming::SubscriptionResponse(_) => eprintln!("Subscription confirmed"),
                    _ => {}
                },
            }
        }

        Ok(())
    }
}

/// Subscribe to best bid/offer updates.
///
/// # Example
///
/// ```bash
/// hypecli subscribe bbo --asset BTC
/// hypecli subscribe bbo --asset PURR/USDC
/// hypecli subscribe bbo --asset xyz:BTC --format json
/// ```
#[derive(Args)]
pub struct BboCmd {
    /// Asset name. Formats:
    /// - "BTC" for BTC perpetual
    /// - "PURR/USDC" for PURR spot market
    /// - "xyz:BTC" for BTC perpetual on xyz HIP3 DEX
    #[arg(long)]
    pub asset: String,
    /// Target chain
    #[arg(long, default_value = "Mainnet")]
    pub chain: Chain,
    /// Output format
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl BboCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let resolved = resolve_asset_for_subscription(&client, &self.asset).await?;

        let core = match self.chain {
            Chain::Mainnet => hypercore::mainnet(),
            Chain::Testnet => hypercore::testnet(),
        };

        let mut ws = core.websocket();
        ws.subscribe(Subscription::Bbo {
            coin: resolved.coin.clone(),
        });

        eprintln!("Subscribing to {} BBO...", self.asset);

        while let Some(event) = ws.next().await {
            match event {
                Event::Connected => eprintln!("Connected"),
                Event::Disconnected => eprintln!("Disconnected, reconnecting..."),
                Event::Message(msg) => match msg {
                    Incoming::Bbo(bbo) => match self.format {
                        OutputFormat::Pretty => {
                            let bid = bbo
                                .bid()
                                .map(|b| format!("{} @ {}", b.sz, b.px))
                                .unwrap_or_else(|| "-".to_string());
                            let ask = bbo
                                .ask()
                                .map(|a| format!("{} @ {}", a.sz, a.px))
                                .unwrap_or_else(|| "-".to_string());
                            let spread = bbo
                                .spread()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "-".to_string());
                            println!(
                                "{}: bid {} | ask {} | spread {}",
                                bbo.coin, bid, ask, spread
                            );
                        }
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string(&bbo)?);
                        }
                    },
                    Incoming::SubscriptionResponse(_) => eprintln!("Subscription confirmed"),
                    _ => {}
                },
            }
        }

        Ok(())
    }
}

/// Subscribe to order book updates (L2 book).
///
/// # Example
///
/// ```bash
/// hypecli subscribe orderbook --asset BTC
/// hypecli subscribe orderbook --asset PURR/USDC --depth 5
/// hypecli subscribe orderbook --asset xyz:BTC
/// ```
#[derive(Args)]
pub struct OrderbookCmd {
    /// Asset name. Formats:
    /// - "BTC" for BTC perpetual
    /// - "PURR/USDC" for PURR spot market
    /// - "xyz:BTC" for BTC perpetual on xyz HIP3 DEX
    #[arg(long)]
    pub asset: String,
    /// Target chain
    #[arg(long, default_value = "Mainnet")]
    pub chain: Chain,
    /// Number of price levels to display (default: 10)
    #[arg(long, default_value = "10")]
    pub depth: usize,
    /// Output format
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl OrderbookCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let resolved = resolve_asset_for_subscription(&client, &self.asset).await?;

        let core = match self.chain {
            Chain::Mainnet => hypercore::mainnet(),
            Chain::Testnet => hypercore::testnet(),
        };

        let mut ws = core.websocket();
        ws.subscribe(Subscription::L2Book {
            coin: resolved.coin.clone(),
        });

        eprintln!("Subscribing to {} orderbook...", self.asset);

        while let Some(event) = ws.next().await {
            match event {
                Event::Connected => eprintln!("Connected"),
                Event::Disconnected => eprintln!("Disconnected, reconnecting..."),
                Event::Message(msg) => match msg {
                    Incoming::L2Book(book) => match self.format {
                        OutputFormat::Pretty => {
                            // Clear screen and print orderbook
                            print!("\x1B[2J\x1B[1;1H");
                            println!("=== {} Orderbook ===\n", book.coin);

                            let mut writer = tabwriter::TabWriter::new(stdout());

                            // Asks (reversed to show best ask at bottom)
                            let asks: Vec<_> = book.levels[1].iter().take(self.depth).collect();
                            writeln!(&mut writer, "ASKS")?;
                            writeln!(&mut writer, "Price\tSize\tOrders")?;
                            for level in asks.iter().rev() {
                                writeln!(&mut writer, "{}\t{}\t{}", level.px, level.sz, level.n)?;
                            }

                            writeln!(&mut writer, "---")?;

                            // Bids
                            writeln!(&mut writer, "BIDS")?;
                            writeln!(&mut writer, "Price\tSize\tOrders")?;
                            for level in book.levels[0].iter().take(self.depth) {
                                writeln!(&mut writer, "{}\t{}\t{}", level.px, level.sz, level.n)?;
                            }

                            writer.flush()?;
                        }
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string(&book)?);
                        }
                    },
                    Incoming::SubscriptionResponse(_) => eprintln!("Subscription confirmed"),
                    _ => {}
                },
            }
        }

        Ok(())
    }
}

/// Subscribe to candlestick (OHLCV) data.
///
/// # Example
///
/// ```bash
/// hypecli subscribe candles --coin BTC --interval 1m
/// hypecli subscribe candles --coin ETH --interval 15m --format json
/// ```
#[derive(Args)]
pub struct CandlesCmd {
    /// Coin symbol (e.g., BTC, ETH)
    #[arg(long)]
    pub coin: String,
    /// Candle interval (1m, 3m, 5m, 15m, 30m, 1h, 2h, 4h, 8h, 12h, 1d, 3d, 1w, 1M)
    #[arg(long, default_value = "1m")]
    pub interval: String,
    /// Target chain
    #[arg(long, default_value = "Mainnet")]
    pub chain: Chain,
    /// Output format
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl CandlesCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let core = match self.chain {
            Chain::Mainnet => hypercore::mainnet(),
            Chain::Testnet => hypercore::testnet(),
        };

        let mut ws = core.websocket();
        ws.subscribe(Subscription::Candle {
            coin: self.coin.clone(),
            interval: self.interval.clone(),
        });

        eprintln!("Subscribing to {} {} candles...", self.coin, self.interval);

        while let Some(event) = ws.next().await {
            match event {
                Event::Connected => eprintln!("Connected"),
                Event::Disconnected => eprintln!("Disconnected, reconnecting..."),
                Event::Message(msg) => match msg {
                    Incoming::Candle(candle) => match self.format {
                        OutputFormat::Pretty => {
                            let change = candle.close - candle.open;
                            let change_pct = if !candle.open.is_zero() {
                                (change / candle.open) * Decimal::ONE_HUNDRED
                            } else {
                                Decimal::ZERO
                            };
                            let sign = if change.is_sign_positive() { "+" } else { "" };
                            println!(
                                "{} {} | O:{} H:{} L:{} C:{} | V:{} | {}{} ({}{:.2}%)",
                                candle.coin,
                                candle.interval,
                                candle.open,
                                candle.high,
                                candle.low,
                                candle.close,
                                candle.volume,
                                sign,
                                change,
                                sign,
                                change_pct
                            );
                        }
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string(&candle)?);
                        }
                    },
                    Incoming::SubscriptionResponse(_) => eprintln!("Subscription confirmed"),
                    _ => {}
                },
            }
        }

        Ok(())
    }
}

/// Subscribe to mid prices for all markets.
///
/// # Example
///
/// ```bash
/// hypecli subscribe all-mids
/// hypecli subscribe all-mids --dex hyperliquid
/// hypecli subscribe all-mids --filter BTC,ETH
/// ```
#[derive(Args)]
pub struct AllMidsCmd {
    /// Optional DEX name to filter
    #[arg(long)]
    pub dex: Option<String>,
    /// Optional comma-separated list of coins to filter
    #[arg(long)]
    pub filter: Option<String>,
    /// Target chain
    #[arg(long, default_value = "Mainnet")]
    pub chain: Chain,
    /// Output format
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl AllMidsCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let core = match self.chain {
            Chain::Mainnet => hypercore::mainnet(),
            Chain::Testnet => hypercore::testnet(),
        };

        let mut ws = core.websocket();
        ws.subscribe(Subscription::AllMids {
            dex: self.dex.clone(),
        });

        let filter_coins: Option<Vec<String>> = self
            .filter
            .as_ref()
            .map(|f| f.split(',').map(|s| s.trim().to_uppercase()).collect());

        eprintln!("Subscribing to all mid prices...");

        while let Some(event) = ws.next().await {
            match event {
                Event::Connected => eprintln!("Connected"),
                Event::Disconnected => eprintln!("Disconnected, reconnecting..."),
                Event::Message(msg) => match msg {
                    Incoming::AllMids { dex, mids } => match self.format {
                        OutputFormat::Pretty => {
                            let dex_str = dex.as_deref().unwrap_or("all");
                            let mut filtered_mids: Vec<_> = mids
                                .iter()
                                .filter(|(coin, _)| {
                                    filter_coins.as_ref().map_or_else(
                                        || true,
                                        |f| f.iter().any(|fc| coin.to_uppercase().contains(fc)),
                                    )
                                })
                                .collect();
                            filtered_mids.sort_by(|a, b| a.0.cmp(b.0));

                            println!("--- Mid Prices ({}) ---", dex_str);
                            for (coin, price) in filtered_mids {
                                println!("{}: {}", coin, price);
                            }
                            println!();
                        }
                        OutputFormat::Json => {
                            let output = serde_json::json!({
                                "dex": dex,
                                "mids": mids
                            });
                            println!("{}", serde_json::to_string(&output)?);
                        }
                    },
                    Incoming::SubscriptionResponse(_) => eprintln!("Subscription confirmed"),
                    _ => {}
                },
            }
        }

        Ok(())
    }
}

/// Subscribe to order updates for a user.
///
/// # Example
///
/// ```bash
/// hypecli subscribe order-updates --user 0x1234...
/// ```
#[derive(Args)]
pub struct OrderUpdatesCmd {
    /// User address to monitor
    #[arg(long)]
    pub user: Address,
    /// Target chain
    #[arg(long, default_value = "Mainnet")]
    pub chain: Chain,
    /// Output format
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl OrderUpdatesCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let core = match self.chain {
            Chain::Mainnet => hypercore::mainnet(),
            Chain::Testnet => hypercore::testnet(),
        };

        let mut ws = core.websocket();
        ws.subscribe(Subscription::OrderUpdates { user: self.user });

        eprintln!("Subscribing to order updates for {}...", self.user);

        while let Some(event) = ws.next().await {
            match event {
                Event::Connected => eprintln!("Connected"),
                Event::Disconnected => eprintln!("Disconnected, reconnecting..."),
                Event::Message(msg) => match msg {
                    Incoming::OrderUpdates(updates) => {
                        for update in updates {
                            match self.format {
                                OutputFormat::Pretty => {
                                    println!(
                                        "[{}] {} {} {} @ {} | status: {:?} | oid: {}",
                                        update.status_timestamp,
                                        update.order.coin,
                                        update.order.side,
                                        update.order.sz,
                                        update.order.limit_px,
                                        update.status,
                                        update.order.oid
                                    );
                                }
                                OutputFormat::Json => {
                                    println!("{}", serde_json::to_string(&update)?);
                                }
                            }
                        }
                    }
                    Incoming::SubscriptionResponse(_) => eprintln!("Subscription confirmed"),
                    _ => {}
                },
            }
        }

        Ok(())
    }
}

/// Subscribe to fill events for a user.
///
/// # Example
///
/// ```bash
/// hypecli subscribe fills --user 0x1234...
/// ```
#[derive(Args)]
pub struct FillsCmd {
    /// User address to monitor
    #[arg(long)]
    pub user: Address,
    /// Target chain
    #[arg(long, default_value = "Mainnet")]
    pub chain: Chain,
    /// Output format
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl FillsCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let core = match self.chain {
            Chain::Mainnet => hypercore::mainnet(),
            Chain::Testnet => hypercore::testnet(),
        };

        let mut ws = core.websocket();
        ws.subscribe(Subscription::UserFills { user: self.user });

        eprintln!("Subscribing to fills for {}...", self.user);

        while let Some(event) = ws.next().await {
            match event {
                Event::Connected => eprintln!("Connected"),
                Event::Disconnected => eprintln!("Disconnected, reconnecting..."),
                Event::Message(msg) => match msg {
                    Incoming::UserFills { user, fills } => {
                        for fill in fills {
                            match self.format {
                                OutputFormat::Pretty => {
                                    println!(
                                        "[{}] {} {} {} @ {} | fee: {} | oid: {}",
                                        fill.time,
                                        fill.coin,
                                        fill.side,
                                        fill.sz,
                                        fill.px,
                                        fill.fee,
                                        fill.oid
                                    );
                                }
                                OutputFormat::Json => {
                                    let output = serde_json::json!({
                                        "user": user,
                                        "fill": fill
                                    });
                                    println!("{}", serde_json::to_string(&output)?);
                                }
                            }
                        }
                    }
                    Incoming::SubscriptionResponse(_) => eprintln!("Subscription confirmed"),
                    _ => {}
                },
            }
        }

        Ok(())
    }
}
