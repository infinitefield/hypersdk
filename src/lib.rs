//! # hypersdk
//!
//! A comprehensive Rust SDK for interacting with the [Hyperliquid](https://hyperliquid.xyz) protocol.
//!
//! Hyperliquid is a high-performance decentralized exchange with two main components:
//! - **HyperCore**: The native L1 chain with perpetual and spot markets
//! - **HyperEVM**: An Ethereum-compatible layer for DeFi integrations
//!
//! ## Quick Navigation
//!
//! | Module | Description | Common Use Cases |
//! |--------|-------------|------------------|
//! | [`hypercore`] | L1 trading & data | Place orders, query markets, stream data |
//! | [`hypercore::http`] | HTTP API client | Account queries, order placement |
//! | [`hypercore::ws`] | WebSocket streaming | Real-time market data, order updates |
//! | [`hypercore::types`] | Core type definitions | Orders, trades, candles, subscriptions |
//! | [`hypercore::signing`] | Signature utilities | Sign actions, recover addresses |
//! | [`hyperevm::morpho`] | Morpho lending | Query APY, lending positions |
//! | [`hyperevm::uniswap`] | Uniswap V3 | Pool prices, liquidity positions |
//!
//! ## Features
//!
//! - Full HyperCore API support (HTTP and WebSocket)
//! - Trading operations (orders, cancellations, modifications)
//! - Real-time market data via WebSocket subscriptions
//! - Asset transfers between perps, spot, and EVM
//! - HyperEVM contract interactions (Morpho, Uniswap)
//! - Type-safe EIP-712 signing for all operations
//! - Accurate price tick rounding for orders
//! - HIP-3 support for multi-DEX perpetuals
//! - Multi-signature transaction support
//!
//! ## Getting Started
//!
//! ### Installation
//!
//! Add to your `Cargo.toml`:
//! ```toml
//! [dependencies]
//! hypersdk = "0.1"
//! rust_decimal = "1.39"
//! tokio = { version = "1", features = ["full"] }
//! anyhow = "1"
//! ```
//!
//! ### Your First Query
//!
//! ```no_run
//! use hypersdk::hypercore;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let client = hypercore::mainnet();
//!     let markets = client.perps().await?;
//!
//!     for market in markets {
//!         println!("{}: {}x leverage", market.name, market.max_leverage);
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ### Placing an Order
//!
//! ```no_run
//! use hypersdk::hypercore::{self, types::*, PrivateKeySigner};
//! use rust_decimal::dec;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let client = hypercore::mainnet();
//! let signer: PrivateKeySigner = "your_private_key".parse()?;
//!
//! let order = BatchOrder {
//!     orders: vec![OrderRequest {
//!         asset: 0, // BTC
//!         is_buy: true,
//!         limit_px: dec!(50000),
//!         sz: dec!(0.1),
//!         reduce_only: false,
//!         order_type: OrderTypePlacement::Limit {
//!             tif: TimeInForce::Gtc,
//!         },
//!         cloid: Default::default(),
//!     }],
//!     grouping: OrderGrouping::Na,
//!     builder: None,
//! };
//!
//! let nonce = chrono::Utc::now().timestamp_millis() as u64;
//! let result = client.place(&signer, order, nonce, None, None).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### WebSocket Subscriptions
//!
//! ```no_run
//! use hypersdk::hypercore::{self, types::*};
//! use futures::StreamExt;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let mut ws = hypercore::mainnet_ws();
//!
//! // Subscribe to market data
//! ws.subscribe(Subscription::Trades { coin: "BTC".into() });
//! ws.subscribe(Subscription::L2Book { coin: "ETH".into() });
//! ws.subscribe(Subscription::Candle {
//!     coin: "BTC".into(),
//!     interval: "15m".into()
//! });
//!
//! // Process incoming messages
//! while let Some(msg) = ws.next().await {
//!     match msg {
//!         Incoming::Trades(trades) => {
//!             for trade in trades {
//!                 println!("Trade: {} @ {}", trade.sz, trade.px);
//!             }
//!         }
//!         Incoming::L2Book(book) => {
//!             println!("Book update for {}", book.coin);
//!         }
//!         Incoming::Candle(candle) => {
//!             println!("Candle: O:{} H:{} L:{} C:{}",
//!                 candle.open, candle.high, candle.low, candle.close);
//!         }
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │                   hypersdk                      │
//! ├─────────────────────┬───────────────────────────┤
//! │    HyperCore L1     │      HyperEVM             │
//! ├─────────────────────┼───────────────────────────┤
//! │ • HTTP Client       │ • Morpho (Lending)        │
//! │ • WebSocket Client  │ • Uniswap V3 (DEX)        │
//! │ • Order Management  │ • ERC-20 Interactions     │
//! │ • Market Data       │                           │
//! │ • Transfers         │                           │
//! │ • EIP-712 Signing   │                           │
//! └─────────────────────┴───────────────────────────┘
//! ```
//!
//! ## Architecture Decisions
//!
//! ### Why `impl Future` instead of `async fn`?
//!
//! The SDK uses `impl Future<Output = Result<...>> + Send + 'static` for many methods
//! instead of the more common `async fn`. This allows you to spawn futures directly:
//!
//! ```ignore
//! // Direct spawning works!
//! tokio::spawn(client.place(order));
//!
//! // Or deferred spawning
//! let future = client.place(order);
//! tokio::spawn(async move { future.await });
//! ```
//!
//! With `async fn`, the compiler cannot guarantee the future is `Send + 'static` when
//! it captures `&self`, preventing use with `tokio::spawn`. See the [README](https://github.com/infinitefield/hypersdk#design-choices)
//! for detailed explanation with playground links.
//!
//! ### High-Precision Decimals
//!
//! All prices and quantities use [`rust_decimal::Decimal`] for precise financial calculations.
//! This avoids floating-point rounding errors that are critical in trading applications.
//!
//! ### Zero-Copy WebSocket
//!
//! The SDK uses [yawc](https://docs.rs/yawc) for WebSocket connections, providing:
//! - Zero-copy message parsing
//! - Per-message deflate compression
//! - Automatic reconnection with subscription management
//!
//! ## Testing
//!
//! Use testnet for development and testing:
//!
//! ```no_run
//! use hypersdk::hypercore;
//!
//! let client = hypercore::testnet();
//! let mut ws = hypercore::testnet_ws();
//! ```
//!
//! ## Examples
//!
//! The [`examples/`](https://github.com/infinitefield/hypersdk/tree/main/examples) directory
//! contains comprehensive examples covering:
//!
//! - Market data queries and WebSocket streaming
//! - Order placement, modification, and cancellation
//! - Asset transfers between perps, spot, and EVM
//! - Multi-signature transactions
//! - HyperEVM interactions (Morpho, Uniswap)
//!
//! ## Modules
//!
//! - [`hypercore`]: HyperCore L1 chain interactions (trading, transfers, WebSocket)
//!   - [`hypercore::http`]: HTTP API client for queries and trading
//!   - [`hypercore::ws`]: WebSocket client for real-time data
//!   - [`hypercore::types`]: Core type definitions (orders, trades, market data)
//! - [`hyperevm`]: HyperEVM contract interactions
//!   - [`hyperevm::morpho`]: Morpho lending protocol integration
//!   - [`hyperevm::uniswap`]: Uniswap V3 DEX integration

pub mod hypercore;
pub mod hyperevm;

/// Re-exported Ethereum address type from Alloy.
///
/// Used throughout the SDK for representing Ethereum-compatible addresses.
pub use alloy::primitives::{Address, U160, U256, address};
/// Re-exported decimal type from rust_decimal.
///
/// Used for precise numerical operations, especially for prices and quantities.
pub use rust_decimal::{Decimal, dec};
