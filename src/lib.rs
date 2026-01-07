//! # hypersdk
//!
//! A comprehensive Rust SDK for interacting with the Hyperliquid protocol.
//!
//! Hyperliquid is a high-performance decentralized exchange with two main components:
//! - **HyperCore**: The native L1 chain with perpetual and spot markets
//! - **HyperEVM**: An Ethereum-compatible layer for DeFi integrations
//!
//! ## Features
//!
//! - Full HyperCore API support (HTTP and WebSocket)
//! - Trading operations (orders, cancellations, modifications)
//! - Real-time market data via WebSocket subscriptions
//! - Asset transfers between perps, spot, and EVM
//! - HyperEVM contract interactions (Morpho, Uniswap)
//! - Type-safe EIP-712 signing for all operations
//!
//! ## Quick Start
//!
//! ### HyperCore - Place an Order
//!
//! ```no_run
//! use hypersdk::hypercore::{self, types::*};
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Create a mainnet client
//! let client = hypercore::mainnet();
//!
//! // Get available markets
//! let perps = client.perps().await?;
//! let spot = client.spot().await?;
//!
//! // Query user balances
//! let balances = client.user_balances(your_address).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### HyperCore - WebSocket Subscriptions
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
//!
//! // Process incoming messages
//! while let Some(msg) = ws.next().await {
//!     match msg {
//!         Incoming::Trades(trades) => println!("Trades: {:?}", trades),
//!         Incoming::L2Book(book) => println!("Order book: {:?}", book),
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ### HyperEVM - Morpho Lending
//!
//! ```no_run
//! use hypersdk::hyperevm::morpho;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let client = morpho::Client::mainnet().await?;
//!
//! // Get APY for a specific market
//! let apy = client.apy(morpho_address, market_id).await?;
//! println!("Borrow APY: {:.2}%", apy.borrow * 100.0);
//! println!("Supply APY: {:.2}%", apy.supply * 100.0);
//! # Ok(())
//! # }
//! ```
//!
//! ### HyperEVM - Uniswap V3
//!
//! ```no_run
//! use hypersdk::hyperevm::uniswap;
//! use rust_decimal_macros::dec;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let contracts = uniswap::Contracts {
//!     factory: "0x...".parse()?,
//!     quoter: "0x...".parse()?,
//!     swap_router: "0x...".parse()?,
//!     non_fungible_position_manager: "0x...".parse()?,
//! };
//!
//! let client = uniswap::Client::mainnet(contracts).await?;
//!
//! // Get pool price
//! let price = client.get_pool_price(token0, token1, 3000).await?;
//! println!("Pool price: {}", price);
//! # Ok(())
//! # }
//! ```
//!
//! ## Modules
//!
//! - [`hypercore`]: HyperCore L1 chain interactions (trading, transfers, WebSocket)
//! - [`hyperevm`]: HyperEVM contract interactions (Morpho, Uniswap)

pub mod hypercore;
pub mod hyperevm;

/// Re-exported Ethereum address type from Alloy.
///
/// Used throughout the SDK for representing Ethereum-compatible addresses.
pub use alloy::primitives::{Address, U160, U256, address};
/// Re-exported decimal type from rust_decimal.
///
/// Used for precise numerical operations, especially for prices and quantities.
pub use rust_decimal::Decimal;
