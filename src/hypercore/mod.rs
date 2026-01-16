//! HyperCore L1 chain interactions.
//!
//! This module provides functionality for interacting with Hyperliquid's native L1 chain,
//! including trading operations, market data queries, and asset transfers.
//!
//! # Components
//!
//! - [`HttpClient`]: HTTP client for API interactions (orders, queries, transfers)
//! - [`WebSocket`]: Real-time WebSocket connection for market data and order updates
//! - Market types: [`PerpMarket`], [`SpotMarket`], [`SpotToken`]
//! - Order types and operations in the [`types`] module
//!
//! # Examples
//!
//! ## Query Markets
//!
//! ```no_run
//! use hypersdk::hypercore;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let client = hypercore::mainnet();
//!
//! // Get perpetual markets
//! let perps = client.perps().await?;
//! for market in perps {
//!     println!("{}: max leverage {}x", market.name, market.max_leverage);
//! }
//!
//! // Get spot markets
//! let spots = client.spot().await?;
//! for market in spots {
//!     println!("{}", market.symbol());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## WebSocket Market Data
//!
//! ```no_run
//! use hypersdk::hypercore::{self, types::*};
//! use futures::StreamExt;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let mut ws = hypercore::mainnet_ws();
//!
//! // Subscribe to trades and order book
//! ws.subscribe(Subscription::Trades { coin: "BTC".into() });
//! ws.subscribe(Subscription::L2Book { coin: "BTC".into() });
//!
//! while let Some(msg) = ws.next().await {
//!     match msg {
//!         Incoming::Trades(trades) => {
//!             for trade in trades {
//!                 println!("{} @ {} size {}", trade.side, trade.px, trade.sz);
//!             }
//!         }
//!         Incoming::L2Book(book) => {
//!             println!("Book update for {}", book.coin);
//!         }
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Place Orders
//!
//! ```no_run
//! use hypersdk::hypercore::{self, types::*, PrivateKeySigner};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let client = hypercore::mainnet();
//! let signer: PrivateKeySigner = "your_private_key".parse()?;
//!
//! // Example order placement - requires dec!() macro and timestamp
//! // See the crate documentation for complete examples
//! # Ok(())
//! # }
//! ```

pub mod error;
pub mod http;
pub mod signing;
pub mod types;
mod utils;
pub mod ws;

use std::{
    fmt,
    hash::Hash,
    sync::atomic::{self, AtomicU64},
};

/// Reimport signers.
pub use alloy::signers::local::PrivateKeySigner;
use alloy::{
    dyn_abi::Eip712Domain,
    primitives::{B128, U256, address},
};
use anyhow::Context;
use chrono::Utc;
use either::Either;
/// Re-export error types.
pub use error::{ActionError, Error};
use reqwest::IntoUrl;
use rust_decimal::{Decimal, MathematicalOps, RoundingStrategy, prelude::ToPrimitive};
use serde::{Deserialize, Serialize};
/// Re-import types.
pub use types::*;
use url::Url;

use crate::{
    Address,
    hyperevm::{from_wei, to_wei},
};

/// Client order ID (cloid).
///
/// A 128-bit identifier that clients can assign to their orders for tracking purposes.
/// This allows you to reference orders by your own ID instead of the exchange-assigned order ID.
pub type Cloid = B128;

/// Order identifier that can be either an exchange-assigned order ID or a client order ID.
///
/// - `Left(u64)`: Exchange-assigned order ID (oid)
/// - `Right(Cloid)`: Client-assigned order ID (cloid)
pub type OidOrCloid = Either<u64, Cloid>;

/// Re-export of the HTTP client for HyperCore API interactions.
///
/// Use this client for placing orders, querying balances, and managing positions.
pub use http::Client as HttpClient;
/// Re-export of the WebSocket connection for real-time market data.
///
/// Use this for subscribing to trades, order books, and order updates.
pub use ws::Connection as WebSocket;

/// Thread-safe nonce generator for Hyperliquid transactions.
///
/// Hyperliquid requires each transaction to have a unique, monotonically increasing nonce
/// to prevent replay attacks. This handler generates nonces based on the current timestamp
/// in milliseconds, ensuring uniqueness even under high-frequency trading scenarios.
///
/// # Thread Safety
///
/// This struct uses atomic operations and is safe to share across threads. Multiple threads
/// can call `next()` concurrently without external synchronization.
///
/// # Nonce Generation Strategy
///
/// 1. Starts with the current timestamp in milliseconds
/// 2. Increments by 1 for each subsequent call
/// 3. If the nonce falls behind the current time by more than 300ms, jumps to current time
/// 4. This ensures nonces stay close to real time while maintaining uniqueness
///
/// # Example
///
/// ```
/// use hypersdk::hypercore::NonceHandler;
///
/// let handler = NonceHandler::default();
///
/// // Generate sequential nonces
/// let nonce1 = handler.next();
/// let nonce2 = handler.next();
/// assert!(nonce2 > nonce1);
///
/// // Nonces are always unique and increasing
/// for _ in 0..1000 {
///     let n = handler.next();
///     assert!(n > nonce1);
/// }
/// ```
///
/// # Use with HTTP Client
///
/// ```no_run
/// use hypersdk::hypercore::{self, NonceHandler};
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = hypercore::mainnet();
/// let nonce_handler = NonceHandler::default();
///
/// // Use for multiple transactions
/// let nonce1 = nonce_handler.next();
/// let nonce2 = nonce_handler.next();
/// # Ok(())
/// # }
/// ```
pub struct NonceHandler {
    nonce: AtomicU64,
}

impl Default for NonceHandler {
    fn default() -> Self {
        let now = Utc::now().timestamp_millis() as u64;
        Self {
            nonce: AtomicU64::new(now),
        }
    }
}

impl NonceHandler {
    /// Generates the next unique nonce for a transaction.
    ///
    /// This method is thread-safe and can be called concurrently from multiple threads.
    /// It guarantees that:
    /// - Each returned nonce is unique
    /// - Nonces are monotonically increasing
    /// - Nonces stay reasonably close to the current timestamp
    ///
    /// # Algorithm
    ///
    /// 1. Gets the current time in milliseconds
    /// 2. Atomically increments the internal nonce counter
    /// 3. If the counter has fallen behind current time by >300ms, resets to current time
    /// 4. Otherwise returns the incremented counter value
    ///
    /// # Returns
    ///
    /// A unique nonce suitable for use in Hyperliquid transactions.
    ///
    /// # Example
    ///
    /// ```
    /// use std::sync::Arc;
    /// use hypersdk::hypercore::NonceHandler;
    ///
    /// let handler = Arc::new(NonceHandler::default());
    /// let nonce = handler.next();
    /// println!("Transaction nonce: {}", nonce);
    /// ```
    pub fn next(&self) -> u64 {
        let now = Utc::now().timestamp_millis() as u64;

        let prev = self.nonce.load(atomic::Ordering::Relaxed);
        if prev + 300 < now {
            self.nonce.fetch_max(now, atomic::Ordering::Relaxed);
        }

        self.nonce.fetch_add(1, atomic::Ordering::Relaxed)
    }
}

/// Chain identifier for Hyperliquid operations.
///
/// This determines which network-specific constants to use for signatures and operations.
///
/// # Serialization
///
/// Serializes to PascalCase format: "Mainnet" or "Testnet".
/// This format is required by the Hyperliquid API.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    derive_more::Display,
    derive_more::FromStr,
    derive_more::IsVariant,
)]
#[serde(rename_all = "PascalCase")]
pub enum Chain {
    /// Mainnet chain
    #[display("Mainnet")]
    Mainnet,
    /// Testnet chain
    #[display("Testnet")]
    Testnet,
}

impl Chain {
    /// Returns the Arbitrum chain ID for EIP-712 signatures based on the chain.
    ///
    /// This method returns the appropriate Arbitrum chain ID to use in EIP-712 signature
    /// domains for actions like USDC transfers, spot sends, and asset transfers.
    ///
    /// # Returns
    ///
    /// - Mainnet: `"0xa4b1"` (Arbitrum One mainnet chain ID)
    /// - Testnet: `"0x66eee"` (Hyperliquid testnet chain ID)
    ///
    /// # Example
    ///
    /// ```rust
    /// use hypersdk::hypercore::Chain;
    ///
    /// let mainnet = Chain::Mainnet;
    /// assert_eq!(mainnet.arbitrum_id(), "0xa4b1");
    ///
    /// let testnet = Chain::Testnet;
    /// assert_eq!(testnet.arbitrum_id(), "0x66eee");
    /// ```
    pub fn arbitrum_id(&self) -> &'static str {
        if self.is_mainnet() {
            ARBITRUM_MAINNET_CHAIN_ID
        } else {
            ARBITRUM_TESTNET_CHAIN_ID
        }
    }

    /// Returns the EIP-712 domain for this chain.
    ///
    /// The domain is used for EIP-712 typed data signatures in cross-chain operations.
    /// Returns the appropriate domain based on whether this is mainnet or testnet.
    ///
    /// # Returns
    ///
    /// - [`ARBITRUM_MAINNET_EIP712_DOMAIN`] for mainnet chains
    /// - [`ARBITRUM_TESTNET_EIP712_DOMAIN`] for testnet chains
    ///
    /// # Example
    ///
    /// ```rust
    /// use hypersdk::hypercore::Chain;
    ///
    /// let mainnet_domain = Chain::Mainnet.domain();
    /// let testnet_domain = Chain::Testnet.domain();
    /// ```
    pub fn domain(&self) -> Eip712Domain {
        if self.is_mainnet() {
            ARBITRUM_MAINNET_EIP712_DOMAIN
        } else {
            ARBITRUM_TESTNET_EIP712_DOMAIN
        }
    }
}

/// Arbitrum One mainnet chain ID for EIP-712 signatures.
///
/// This chain ID is used in EIP-712 signature domains for cross-chain operations
/// involving Arbitrum, such as:
/// - USDC transfers between HyperCore and Arbitrum
/// - Spot token transfers
/// - Asset transfers to/from HyperEVM
///
/// # Value
///
/// `0xa4b1` - The standard Arbitrum One mainnet chain ID (decimal: 42161)
///
/// # Usage
///
/// This constant should be used as the `signature_chain_id` field when creating
/// actions like `UsdSend`, `SpotSend`, or `SendAsset` on mainnet.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore::{ARBITRUM_MAINNET_CHAIN_ID, types::UsdSend, Chain};
///
/// // Example UsdSend construction - requires dec!() macro
/// // let usd_send = UsdSend { ... };
/// ```
///
/// # See Also
///
/// - [`ARBITRUM_TESTNET_CHAIN_ID`]: For testnet operations
/// - [`Chain::arbitrum_id()`]: Helper method to get the correct ID based on chain
pub const ARBITRUM_MAINNET_CHAIN_ID: &str = "0xa4b1";

/// Hyperliquid testnet chain ID for EIP-712 signatures.
///
/// This chain ID is used in EIP-712 signature domains for testnet operations
/// involving asset transfers and cross-chain operations on Hyperliquid testnet.
///
/// # Value
///
/// `0x66eee` - The Hyperliquid testnet chain ID (decimal: 421614)
///
/// # Important Notes
///
/// - This is **not** the Arbitrum testnet chain ID
/// - This is Hyperliquid's custom chain ID for testnet operations
/// - Also used for multisig operations on both mainnet and testnet
///
/// # Usage
///
/// This constant should be used as the `signature_chain_id` field when creating
/// actions like `UsdSend`, `SpotSend`, or `SendAsset` on testnet.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore::{ARBITRUM_TESTNET_CHAIN_ID, types::UsdSend, Chain};
///
/// // Example UsdSend construction - requires dec!() macro
/// // let usd_send = UsdSend { ... };
/// ```
///
/// # See Also
///
/// - [`ARBITRUM_MAINNET_CHAIN_ID`]: For mainnet operations
/// - [`Chain::arbitrum_id()`]: Helper method to get the correct ID based on chain
pub const ARBITRUM_TESTNET_CHAIN_ID: &str = "0x66eee";

/// USDC contract address on HyperEVM.
///
/// Note: This address differs from the one linked in HyperCore documentation.
/// Use this address when interacting with USDC on HyperEVM.
pub const USDC_CONTRACT_IN_EVM: Address = address!("0xb88339CB7199b77E23DB6E890353E22632Ba630f");

/// Creates a mainnet HTTP client for HyperCore.
///
/// This is a convenience function that creates a client pointing to the default mainnet API.
///
/// # Example
///
/// ```
/// use hypersdk::hypercore;
///
/// let client = hypercore::mainnet();
/// ```
#[inline(always)]
pub fn mainnet() -> HttpClient {
    HttpClient::new(Chain::Mainnet)
}

/// Creates a testnet HTTP client for HyperCore.
///
/// This is a convenience function that creates a client pointing to the default testnet API.
///
/// # Example
///
/// ```
/// use hypersdk::hypercore;
///
/// let client = hypercore::testnet();
/// ```
#[inline(always)]
pub fn testnet() -> HttpClient {
    HttpClient::new(Chain::Testnet)
}

/// Creates a mainnet WebSocket connection for HyperCore.
///
/// This is a convenience function that creates a WebSocket connection to the mainnet API.
///
/// # Example
///
/// ```
/// use hypersdk::hypercore;
/// use futures::StreamExt;
///
/// # async fn example() {
/// let mut ws = hypercore::mainnet_ws();
/// // Subscribe to market data
/// # }
/// ```
#[inline(always)]
pub fn mainnet_ws() -> WebSocket {
    WebSocket::new(mainnet_websocket_url())
}

/// Returns the default mainnet HTTP API URL.
///
/// URL: `https://api.hyperliquid.xyz`
#[inline(always)]
pub fn mainnet_url() -> Url {
    "https://api.hyperliquid.xyz".parse().unwrap()
}

/// Returns the default mainnet WebSocket URL.
///
/// URL: `wss://api.hyperliquid.xyz/ws`
#[inline(always)]
pub fn mainnet_websocket_url() -> Url {
    "wss://api.hyperliquid.xyz/ws".parse().unwrap()
}

/// Returns the default testnet HTTP API URL.
///
/// URL: `https://api.hyperliquid-testnet.xyz`
#[inline(always)]
pub fn testnet_url() -> Url {
    "https://api.hyperliquid-testnet.xyz".parse().unwrap()
}

/// Returns the default testnet WebSocket URL.
///
/// URL: `wss://api.hyperliquid-testnet.xyz/ws`
#[inline(always)]
pub fn testnet_websocket_url() -> Url {
    "wss://api.hyperliquid-testnet.xyz/ws".parse().unwrap()
}

/// Creates a testnet WebSocket connection for HyperCore.
///
/// This is a convenience function that creates a WebSocket connection to the testnet API.
///
/// # Example
///
/// ```
/// use hypersdk::hypercore;
/// use futures::StreamExt;
///
/// # async fn example() {
/// let mut ws = hypercore::testnet_ws();
/// // Subscribe to market data
/// # }
/// ```
#[inline(always)]
pub fn testnet_ws() -> WebSocket {
    WebSocket::new(testnet_websocket_url())
}

/// Price tick configuration for determining valid price increments.
///
/// Hyperliquid enforces different tick size constraints for spot and perpetual markets.
/// This struct provides O(1) tick size calculation using a unified significant figures algorithm.
///
/// # Algorithm
///
/// The tick size is calculated to maintain **5 significant figures** while respecting
/// market-specific decimal constraints:
///
/// ```text
/// sig_figs = floor(log10(price)) + 1        // Number of integer digits
/// decimals = 5 - sig_figs                    // Decimal places needed for 5 sig figs
/// max_decimals = clamp(decimals, 0, max_decimals)
/// tick = 10^(-max_decimals)
/// ```
///
/// # Market Types
///
/// ## Spot Markets
/// - **Max decimals**: 8 (max_decimals = 8 - sz_decimals)
/// - Higher `max_decimals` allows finer tick sizes for low-priced assets
/// - Example: PURR/USDC with sz_decimals=0 → max_decimals=8 → tick can be as fine as 10^-8
///
/// ## Perpetual Markets
/// - **Max decimals**: 6 (max_decimals = 6 - sz_decimals)
/// - BTC (sz_decimals=5): max_decimals=1, allows up to 1 decimal place
/// - SOL (sz_decimals=2): max_decimals=4, allows up to 4 decimal places
///
/// # Examples
///
/// ```text
/// BTC perpetual (sz_decimals=5, max_decimals=1):
/// - Price 93231 (5 digits): decimals = 5-5 = 0, clamp(0,0,1) = 0 → tick = 10^0 = 1
/// - Price 93231.23 rounds to 93231
///
/// SOL perpetual (sz_decimals=2, max_decimals=4):
/// - Price 137 (3 digits): decimals = 5-3 = 2, clamp(2,0,4) = 2 → tick = 10^-2 = 0.01
/// - Price 137.23025 rounds to 137.23
///
/// - Price 99 (2 digits): decimals = 5-2 = 3, clamp(3,0,4) = 3 → tick = 10^-3 = 0.001
/// - Price 99.98241 rounds to 99.982
/// ```
///
/// See: <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/tick-and-lot-size>
#[derive(Debug, Clone, Copy)]
pub struct PriceTick {
    /// Maximum decimal places allowed for this market.
    /// - Spot: max_decimals = 8 - sz_decimals
    /// - Perp: max_decimals = 6 - sz_decimals
    max_decimals: i64,
}

impl PriceTick {
    /// Returns the valid tick size for a given price.
    ///
    /// The tick size determines the minimum price increment for orders at this price level.
    /// This implementation maintains 5 significant figures while respecting market-specific
    /// decimal constraints.
    ///
    /// Returns `None` if the price is invalid (zero, negative, or causes calculation overflow).
    ///
    /// # Algorithm
    ///
    /// 1. Calculate number of integer digits: `sig_figs = floor(log10(price)) + 1`
    /// 2. Calculate decimals needed for 5 sig figs: `decimals = 5 - sig_figs`
    /// 3. Clamp to market limits: `max_decimals = clamp(decimals, 0, max_decimals)`
    /// 4. Return tick size: `tick = 10^(-max_decimals)`
    ///
    /// # Examples
    ///
    /// Example: tick_for() calculates tick size based on price.
    /// See the PriceTick documentation for calculation details.
    pub fn tick_for(&self, price: Decimal) -> Option<Decimal> {
        let sig_figs = price.log10();
        let sig_figs_n = sig_figs.ceil().to_i32()? as i64;
        let decimals = 5_i64 - sig_figs_n;
        let max_decimals = decimals.clamp(0, self.max_decimals);
        Some(Decimal::TEN.powi(-max_decimals))
    }

    /// Rounds a price to the nearest valid tick.
    ///
    /// Returns `None` if the price is invalid or cannot be rounded.
    ///
    /// # Example
    ///
    /// Example: round() rounds price to nearest valid tick.
    /// See the PriceTick documentation for rounding details.
    pub fn round(&self, price: Decimal) -> Option<Decimal> {
        let tick = self.tick_for(price)?;
        // Use MidpointTowardZero strategy (round half dow)
        let rounded =
            (price / tick).round_dp_with_strategy(0, RoundingStrategy::MidpointTowardZero) * tick;
        Some(rounded)
    }

    /// Rounds a price to the nearest valid tick based on order side and aggressiveness.
    ///
    /// This method provides directional rounding control to optimize order placement strategy.
    /// Unlike the neutral [`round`](Self::round) method, this allows you to specify whether
    /// you want conservative (safer) or aggressive (more likely to fill) pricing.
    ///
    /// # Parameters
    ///
    /// - `side`: The order side ([`Side::Bid`] for buy orders, [`Side::Ask`] for sell orders)
    /// - `price`: The price to round to a valid tick
    /// - `conservative`: Rounding strategy flag
    ///   - `true`: Conservative rounding (safer for maker, less likely to fill immediately)
    ///   - `false`: Aggressive rounding (favors taker, more likely to fill immediately)
    ///
    /// # Rounding Behavior
    ///
    /// The rounding direction depends on both the order side and the conservative flag:
    ///
    /// | Side | Conservative | Direction | Rationale |
    /// |------|-------------|-----------|-----------|
    /// | Ask (Sell) | `true` | **UP** | Higher sell price → safer for seller, less likely to fill |
    /// | Ask (Sell) | `false` | **DOWN** | Lower sell price → more competitive, more likely to fill |
    /// | Bid (Buy) | `true` | **DOWN** | Lower buy price → safer for buyer, less likely to fill |
    /// | Bid (Buy) | `false` | **UP** | Higher buy price → more competitive, more likely to fill |
    ///
    /// # Use Cases
    ///
    /// **Conservative (Maker Strategy)**
    /// - Use when placing limit orders away from the market price
    /// - Prioritizes better execution price over fill likelihood
    /// - Suitable for market making or accumulating positions over time
    ///
    /// **Aggressive (Taker Strategy)**
    /// - Use when you want orders to fill quickly
    /// - Prioritizes execution certainty over price optimization
    /// - Suitable for closing positions urgently or market taking
    ///
    /// # Returns
    ///
    /// - `Some(Decimal)`: The rounded price at a valid tick
    /// - `None`: If the price is invalid (zero, negative, or causes overflow)
    ///
    /// # Examples
    ///
    /// Example: round_by_side() provides directional rounding.
    /// Conservative rounding favors better price, aggressive favors faster fill.
    /// See the PriceTick documentation for rounding strategy details.
    ///
    /// # See Also
    ///
    /// - [`round`](Self::round): Neutral rounding (midpoint toward zero)
    /// - [`tick_for`](Self::tick_for): Get the tick size for a given price
    pub fn round_by_side(&self, side: Side, price: Decimal, conservative: bool) -> Option<Decimal> {
        let tick = self.tick_for(price)?;
        let strategy = match (side, conservative) {
            (Side::Ask, true) | (Side::Bid, false) => {
                // Round up: higher price for asks (safer), higher price for bids (aggressive)
                RoundingStrategy::ToPositiveInfinity
            }
            (Side::Ask, false) | (Side::Bid, true) => {
                // Round down: lower price for asks (aggressive), lower price for bids (safer)
                RoundingStrategy::ToNegativeInfinity
            }
        };
        let rounded = price.round_dp_with_strategy(tick.scale(), strategy);
        Some(rounded)
    }
}

/// Creates a price tick configuration for a spot market.
///
/// See: <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/tick-and-lot-size>
fn build_price_ticks(sz_decimals: i64) -> PriceTick {
    let max_decimals = 8 - sz_decimals;
    PriceTick { max_decimals }
}

/// Creates a price tick configuration for a perpetual market.
///
/// For perps, the max significant figures is 5 and max decimal places is 6.
/// This function uses: max_decimals = 6 - sz_decimals (as a construction parameter)
///
/// See: <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/tick-and-lot-size>
fn build_perp_price_ticks(sz_decimals: i64) -> PriceTick {
    let max_decimals = 6 - sz_decimals;
    PriceTick { max_decimals }
}

/// Perpetual futures contract market.
///
/// Represents a perpetual (non-expiring) futures contract on Hyperliquid.
/// Perpetual contracts allow traders to speculate on price movements with leverage.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = hypercore::mainnet();
/// let perps = client.perps().await?;
///
/// for market in perps {
///     println!("{}: {}x leverage, {} collateral",
///         market.name, market.max_leverage, market.collateral.name);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct PerpMarket {
    /// Market name (e.g., "BTC", "ETH")
    pub name: String,
    /// Market index used in API calls
    pub index: usize,
    /// Number of decimal places supported for sizes
    pub sz_decimals: i64,
    /// Collateral token used for this market (typically USDC)
    pub collateral: SpotToken,
    /// Maximum allowed leverage for this market
    pub max_leverage: u64,
    /// Whether margin is isolated
    pub isolated_margin: bool,
    /// Margin mode for this market
    pub margin_mode: Option<MarginMode>,
    /// Price tick configuration for valid price increments
    pub table: PriceTick,
}

impl PerpMarket {
    /// Returns the market symbol (same as name for perps).
    #[must_use]
    pub fn symbol(&self) -> &str {
        &self.name
    }

    /// Returns the price tick configuration for this market.
    #[must_use]
    pub fn tick_table(&self) -> &PriceTick {
        &self.table
    }

    /// Returns the tick size for a given price in this market.
    ///
    /// See [`PriceTick::tick_for`] for details on the calculation.
    pub fn tick_for(&self, price: Decimal) -> Option<Decimal> {
        self.table.tick_for(price)
    }

    /// Rounds a price to the nearest valid tick for this market.
    ///
    /// Uses midpoint-toward-zero rounding strategy (round half down).
    ///
    /// Returns `None` if the price is invalid.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use hypersdk::hypercore::PerpMarket;
    /// # let market: PerpMarket = unimplemented!();
    /// // Example: round_price() rounds to nearest valid tick
    /// // let rounded = market.round_price(price);
    /// ```
    pub fn round_price(&self, price: Decimal) -> Option<Decimal> {
        self.table.round(price)
    }

    /// Rounds a price based on order side and trading strategy.
    ///
    /// This method provides directional rounding to optimize order placement. Use conservative
    /// rounding for limit orders (better price, less likely to fill) or aggressive rounding
    /// for market-taking orders (worse price, more likely to fill).
    ///
    /// # Parameters
    ///
    /// - `side`: Order side ([`Side::Bid`] for buy, [`Side::Ask`] for sell)
    /// - `price`: The price to round
    /// - `conservative`: Rounding strategy
    ///   - `true`: Conservative (maker strategy, better price)
    ///   - `false`: Aggressive (taker strategy, faster fill)
    ///
    /// # Rounding Direction
    ///
    /// | Side | Conservative | Direction | Use Case |
    /// |------|-------------|-----------|----------|
    /// | Ask | `true` | UP | Limit sell away from market |
    /// | Ask | `false` | DOWN | Aggressive sell / market take |
    /// | Bid | `true` | DOWN | Limit buy away from market |
    /// | Bid | `false` | UP | Aggressive buy / market take |
    ///
    /// # Returns
    ///
    /// - `Some(Decimal)`: Rounded price at a valid tick
    /// - `None`: If price is invalid
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hypersdk::hypercore::{PerpMarket, types::Side};
    /// # let market: PerpMarket = unimplemented!();
    /// // Example: round_by_side() provides directional rounding
    /// // let ask = market.round_by_side(Side::Ask, price, true);
    /// // let bid = market.round_by_side(Side::Bid, price, false);
    /// ```
    ///
    /// # See Also
    ///
    /// - [`PriceTick::round_by_side`]: Detailed explanation of rounding logic
    /// - [`round_price`](Self::round_price): Neutral rounding
    pub fn round_by_side(&self, side: Side, price: Decimal, conservative: bool) -> Option<Decimal> {
        self.table.round_by_side(side, price, conservative)
    }
}

/// Spot market trading pair.
///
/// Represents a spot market where two tokens can be directly exchanged.
/// Each market consists of a base token and a quote token.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = hypercore::mainnet();
/// let spots = client.spot().await?;
///
/// for market in spots {
///     println!("{}: {} / {}",
///         market.name, market.tokens[0].name, market.tokens[1].name);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct SpotMarket {
    /// Market name (e.g., "PURR/USDC", "BTC/USDC")
    pub name: String,
    /// Market index used in API calls (10_000 + spot index)
    pub index: usize,
    /// Base token (first element) and quote token (second element)
    pub tokens: [SpotToken; 2],
    /// Price tick configuration for valid price increments
    pub table: PriceTick,
}

impl SpotMarket {
    /// Returns the trading symbol in "BASE/QUOTE" format.
    #[must_use]
    pub fn symbol(&self) -> String {
        format!("{}/{}", self.tokens[0].name, self.tokens[1].name)
    }

    /// Returns the base token (first token in the pair).
    #[must_use]
    pub fn base(&self) -> &SpotToken {
        &self.tokens[0]
    }

    /// Returns the quote token (second token in the pair).
    #[must_use]
    pub fn quote(&self) -> &SpotToken {
        &self.tokens[1]
    }

    /// Returns the price tick configuration for this market.
    #[must_use]
    pub fn tick_table(&self) -> &PriceTick {
        &self.table
    }

    /// Returns the tick size for a given price in this market.
    ///
    /// See [`PriceTick::tick_for`] for details on the calculation.
    pub fn tick_for(&self, price: Decimal) -> Option<Decimal> {
        self.table.tick_for(price)
    }

    /// Rounds a price to the nearest valid tick for this market.
    ///
    /// Uses midpoint-toward-zero rounding strategy (round half down).
    ///
    /// Returns `None` if the price is invalid.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use hypersdk::hypercore::SpotMarket;
    /// # let market: SpotMarket = unimplemented!();
    /// // Example: round_price() rounds to nearest valid tick
    /// // let rounded = market.round_price(price);
    /// ```
    pub fn round_price(&self, price: Decimal) -> Option<Decimal> {
        self.table.round(price)
    }

    /// Rounds a price based on order side and trading strategy.
    ///
    /// This method provides directional rounding to optimize order placement. Use conservative
    /// rounding for limit orders (better price, less likely to fill) or aggressive rounding
    /// for market-taking orders (worse price, more likely to fill).
    ///
    /// # Parameters
    ///
    /// - `side`: Order side ([`Side::Bid`] for buy, [`Side::Ask`] for sell)
    /// - `price`: The price to round
    /// - `conservative`: Rounding strategy
    ///   - `true`: Conservative (maker strategy, better price)
    ///   - `false`: Aggressive (taker strategy, faster fill)
    ///
    /// # Rounding Direction
    ///
    /// | Side | Conservative | Direction | Use Case |
    /// |------|-------------|-----------|----------|
    /// | Ask | `true` | UP | Limit sell away from market |
    /// | Ask | `false` | DOWN | Aggressive sell / market take |
    /// | Bid | `true` | DOWN | Limit buy away from market |
    /// | Bid | `false` | UP | Aggressive buy / market take |
    ///
    /// # Returns
    ///
    /// - `Some(Decimal)`: Rounded price at a valid tick
    /// - `None`: If price is invalid
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hypersdk::hypercore::{SpotMarket, types::Side};
    /// # let market: SpotMarket = unimplemented!();
    /// // Example: round_by_side() provides directional rounding
    /// // let ask = market.round_by_side(Side::Ask, price, true);
    /// // let bid = market.round_by_side(Side::Bid, price, false);
    /// ```
    ///
    /// # See Also
    ///
    /// - [`PriceTick::round_by_side`]: Detailed explanation of rounding logic
    /// - [`round_price`](Self::round_price): Neutral rounding
    pub fn round_by_side(&self, side: Side, price: Decimal, conservative: bool) -> Option<Decimal> {
        self.table.round_by_side(side, price, conservative)
    }
}

impl PartialEq for SpotMarket {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for SpotMarket {}

#[cfg(test)]
mod tick_tests {
    use rust_decimal::dec;

    use super::*;

    #[test]
    fn test_perp() {
        let prices = vec![
            (5, dec!(93231.23), dec!(1), dec!(93231)),
            (5, dec!(108_234.23), dec!(1), dec!(108_234)),
            (2, dec!(137.23025), dec!(0.01), dec!(137.23)),
            (2, dec!(99.98241), dec!(0.001), dec!(99.982)),
            (0, dec!(0.001234), dec!(0.000001), dec!(0.001234)),
            (0, dec!(0.051618), dec!(0.000001), dec!(0.051618)),
            (0, dec!(0.000829), dec!(0.000001), dec!(0.000829)),
        ];

        for (sz_decimals, price, expected_tick, expected_price) in prices {
            let table = build_perp_price_ticks(sz_decimals);
            let tick = table.tick_for(price);
            assert_eq!(
                tick,
                Some(expected_tick),
                "${}: expected tick {}, got {:?}",
                price,
                expected_tick,
                tick
            );

            let output_price = table.round(price).unwrap();
            assert_eq!(
                expected_price, output_price,
                "${}: expected price {}, got {}",
                price, expected_price, output_price
            );
        }
    }

    #[test]
    fn test_spot() {
        let prices = vec![
            (5, dec!(93231.23), dec!(1), dec!(93231)),
            (5, dec!(108_234.23), dec!(1), dec!(108_234)),
            (2, dec!(137.23025), dec!(0.01), dec!(137.23)),
            (2, dec!(99.98241), dec!(0.001), dec!(99.982)),
            (0, dec!(0.0000003315), dec!(0.00000001), dec!(0.00000033)),
            (2, dec!(0.00001501), dec!(0.000001), dec!(0.000015)),
            (0, dec!(0.9543309), dec!(0.00001), dec!(0.95433)),
            (2, dec!(15.9715981), dec!(0.001), dec!(15.972)),
        ];

        for (sz_decimals, price, expected_tick, expected_price) in prices {
            let table = build_price_ticks(sz_decimals);
            let tick = table.tick_for(price);
            assert_eq!(
                tick,
                Some(expected_tick),
                "${}: expected tick {}, got {:?}",
                price,
                expected_tick,
                tick
            );

            let output_price = table.round(price).unwrap();
            assert_eq!(
                expected_price, output_price,
                "${}: expected price {}, got {}",
                price, expected_price, output_price
            );
        }
    }
}

/// Spot token on HyperCore.
///
/// Represents a token that can be traded on Hyperliquid's spot markets.
/// Tokens may be bridgeable to HyperEVM if they have an EVM contract address.
///
/// # EVM Bridging
///
/// Tokens with `evm_contract` set can be transferred between HyperCore and HyperEVM:
/// - Use `cross_chain_address` as the destination when transferring to EVM
/// - Use the HTTP client's `transfer_to_evm` and `transfer_from_evm` methods
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = hypercore::mainnet();
/// let tokens = client.spot_tokens().await?;
///
/// for token in tokens {
///     if token.is_evm_linked() {
///         println!("{} is bridgeable to EVM at {:?}", token.name, token.evm_contract);
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotToken {
    /// Token name (e.g., "USDC", "BTC", "PURR")
    pub name: String,
    /// Token index in the spot token array
    pub index: u32,
    /// Unique token identifier in HyperCore
    pub token_id: B128,
    /// EVM contract address if the token is bridgeable
    ///
    /// `None` means the token only exists on HyperCore.
    pub evm_contract: Option<Address>,
    /// Cross-chain transfer address for bridging between HyperCore and HyperEVM.
    ///
    /// Use this address as the destination when transferring from Core to EVM.
    ///
    /// **Special case:** HYPE token has no `evm_contract` but has this field set.
    pub cross_chain_address: Option<Address>,
    /// Number of decimal places for sizes in HyperCore
    pub sz_decimals: i64,
    /// Number of decimal places used for wei representation
    pub wei_decimals: i64,
    /// Additional decimal places when represented on EVM.
    ///
    /// Total EVM decimals = `sz_decimals` + `evm_extra_decimals`.
    pub evm_extra_decimals: i64,
}

impl SpotToken {
    /// Converts a decimal amount to wei representation.
    ///
    /// Uses the token's total decimals (wei_decimals + evm_extra_decimals).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use hypersdk::hypercore::SpotToken;
    /// # let token: SpotToken = unimplemented!();
    /// // Example: to_wei() converts decimal to wei representation
    /// // let wei = token.to_wei(amount);
    /// ```
    #[must_use]
    pub fn to_wei(&self, size: Decimal) -> U256 {
        to_wei(size, (self.wei_decimals + self.evm_extra_decimals) as u32)
    }

    /// Converts wei representation to a decimal amount.
    ///
    /// Uses the token's total decimals (wei_decimals + evm_extra_decimals).
    ///
    /// # Example
    ///
    /// ```
    /// # use hypersdk::hypercore::SpotToken;
    /// # use hypersdk::{Address, Decimal, U256};
    /// # let token = SpotToken {
    /// #     name: "USDC".into(), index: 0, token_id: Default::default(),
    /// #     evm_contract: None, cross_chain_address: None,
    /// #     sz_decimals: 6, wei_decimals: 6, evm_extra_decimals: 12
    /// # };
    /// let wei = U256::from(100_500_000_000_000_000_000u128);
    /// let amount = token.from_wei(wei);
    /// ```
    #[must_use]
    pub fn from_wei(&self, size: U256) -> Decimal {
        from_wei(size, (self.wei_decimals + self.evm_extra_decimals) as u32)
    }

    /// Returns whether the token can be bridged to HyperEVM.
    ///
    /// Returns `true` if the token has an EVM contract address.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use hypersdk::hypercore;
    /// # async fn example() -> anyhow::Result<()> {
    /// # let client = hypercore::mainnet();
    /// let tokens = client.spot_tokens().await?;
    /// let evm_tokens: Vec<_> = tokens.into_iter()
    ///     .filter(|t| t.is_evm_linked())
    ///     .collect();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    #[inline(always)]
    pub fn is_evm_linked(&self) -> bool {
        self.evm_contract.is_some()
    }

    /// Returns the total decimals for EVM representation.
    ///
    /// This is the sum of `sz_decimals` and `evm_extra_decimals`.
    #[must_use]
    #[inline(always)]
    pub fn total_evm_decimals(&self) -> i64 {
        self.sz_decimals + self.evm_extra_decimals
    }

    /// Returns the bridge address for cross-chain transfers.
    ///
    /// Returns `None` if the token cannot be bridged.
    #[must_use]
    #[inline(always)]
    pub fn bridge_address(&self) -> Option<Address> {
        self.cross_chain_address
    }
}

impl Hash for SpotToken {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.token_id.hash(state);
    }
}

impl PartialEq for SpotToken {
    fn eq(&self, other: &Self) -> bool {
        self.token_id == other.token_id
    }
}

impl Eq for SpotToken {}

impl fmt::Display for SpotToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

async fn raw_spot_markets(
    core_url: impl IntoUrl,
    client: reqwest::Client,
) -> anyhow::Result<SpotTokens> {
    let mut url = core_url.into_url()?;
    url.set_path("/info");
    let resp = client.post(url).json(&InfoRequest::SpotMeta).send().await?;
    Ok(resp.json().await?)
}

/// Fetches all available spot tokens from HyperCore.
///
/// Returns a list of all tokens that can be traded on spot markets.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
///
/// # async fn example() -> anyhow::Result<()> {
/// let url = hypercore::mainnet_url();
/// let client = reqwest::Client::new();
/// let tokens = hypercore::spot_tokens(url, client).await?;
///
/// for token in tokens {
///     println!("{}: {} decimals", token.name, token.sz_decimals);
/// }
/// # Ok(())
/// # }
/// ```
pub async fn spot_tokens(
    core_url: impl IntoUrl,
    client: reqwest::Client,
) -> anyhow::Result<Vec<SpotToken>> {
    let data = raw_spot_markets(core_url, client).await?;

    let spot_tokens: Vec<_> = data.tokens.iter().cloned().map(SpotToken::from).collect();
    Ok(spot_tokens)
}

/// Fetches all available spot trading markets from HyperCore.
///
/// Returns a list of all spot trading pairs with their associated tokens and price tick tables.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
///
/// # async fn example() -> anyhow::Result<()> {
/// let url = hypercore::mainnet_url();
/// let client = reqwest::Client::new();
/// let markets = hypercore::spot_markets(url, client).await?;
///
/// for market in markets {
///     println!("{}: {} / {}", market.name, market.tokens[0].name, market.tokens[1].name);
/// }
/// # Ok(())
/// # }
/// ```
pub async fn spot_markets(
    core_url: impl IntoUrl,
    client: reqwest::Client,
) -> anyhow::Result<Vec<SpotMarket>> {
    let data = raw_spot_markets(core_url, client).await?;
    let mut markets = Vec::with_capacity(data.universe.len());

    let spot_tokens: Vec<_> = data.tokens.iter().cloned().map(SpotToken::from).collect();

    for item in data.universe {
        let (_, base) = spot_tokens
            .iter()
            .enumerate()
            .find(|(index, _)| *index as u32 == item.tokens[0])
            .expect("base");
        let (_, quote) = spot_tokens
            .iter()
            .enumerate()
            .find(|(index, _)| *index as u32 == item.tokens[1])
            .expect("quote");

        markets.push(SpotMarket {
            name: item.name,
            index: 10_000 + item.index,
            tokens: [base.clone(), quote.clone()],
            table: build_price_ticks(base.sz_decimals),
        });
    }

    Ok(markets)
}

/// Fetches all available perpetual futures DEXes from HyperCore.
///
/// Returns a list of all DEXes that offer perpetual futures trading.
/// This is a standalone function that can be used without creating a client instance.
///
/// # Parameters
///
/// - `core_url`: The HyperCore API URL (use [`mainnet_url()`] or [`testnet_url()`])
/// - `client`: The HTTP client to use for the request
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
///
/// # async fn example() -> anyhow::Result<()> {
/// let url = hypercore::mainnet_url();
/// let client = reqwest::Client::new();
/// let dexes = hypercore::perp_dexs(url, client).await?;
///
/// for dex in dexes {
///     println!("DEX: {}", dex.name());
/// }
/// # Ok(())
/// # }
/// ```
pub async fn perp_dexs(
    core_url: impl IntoUrl,
    client: reqwest::Client,
) -> anyhow::Result<Vec<Dex>> {
    let mut url = core_url.into_url()?;
    url.set_path("/info");

    let resp = client
        .post(url)
        .json(&InfoRequest::PerpDexs)
        .send()
        .await
        .context("info")?;

    let dexes: Vec<Option<PerpDex>> = resp.json().await?;
    let dex_list = dexes
        .into_iter()
        .enumerate()
        .filter_map(|(index, dex)| {
            dex.map(|dex| Dex {
                name: dex.name,
                index,
            })
        })
        .collect();

    Ok(dex_list)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PerpDex {
    name: String,
}

/// Fetches all available perpetual futures markets from HyperCore.
///
/// Returns a list of all perpetual contracts with leverage, collateral, and margin information.
pub async fn perp_markets(
    core_url: impl IntoUrl,
    client: reqwest::Client,
    dex: Option<Dex>,
) -> anyhow::Result<Vec<PerpMarket>> {
    let mut url = core_url.into_url()?;
    url.set_path("/info");

    // get it to gather the collateral token
    let spot = raw_spot_markets(url.clone(), client.clone()).await?;

    let resp = client
        .post(url)
        .json(&InfoRequest::Meta {
            dex: dex.as_ref().map(|dex| dex.name.clone()),
        })
        .send()
        .await
        .context("meta")?;
    let data: PerpTokens = resp.json().await?;
    let collateral = &spot.tokens[data.collateral_token];
    let collateral = SpotToken::from(collateral.clone());
    let dex_index = dex.as_ref().map(|dex| dex.index).unwrap_or_default();

    let perps = data
        .universe
        .into_iter()
        .enumerate()
        .map(|(index, perp)| {
            // https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/asset-ids
            let index = 100_000 * usize::from(dex.is_some()) + dex_index * 10_000 + index;
            PerpMarket {
                name: perp.name,
                index,
                max_leverage: perp.max_leverage,
                sz_decimals: perp.sz_decimals,
                collateral: collateral.clone(),
                isolated_margin: perp.only_isolated,
                margin_mode: perp.margin_mode,
                table: build_perp_price_ticks(perp.sz_decimals),
            }
        })
        .collect();

    Ok(perps)
}

// TODO: perpDexs

/// Generates an EVM transfer address for cross-chain transfers.
///
/// Creates addresses in the format `0x20000000000000000000000000000000000000XX`
/// where XX is the token index, used for transferring tokens from HyperCore to HyperEVM.
fn generate_evm_transfer_address(index: usize) -> Address {
    // Base address: 0x2000000000000000000000000000000000000000
    // The 0x20 prefix identifies these as cross-chain transfer addresses
    let base = U256::from(0x20) << 152; // Shift 0x20 to the first byte (19 bytes * 8 bits = 152)
    let addr: U256 = base + U256::from(index);
    let bytes = addr.to_be_bytes::<32>();
    Address::from_slice(&bytes[12..]) // Take last 20 bytes for Address
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PerpTokens {
    universe: Vec<PerpUniverseItem>,
    collateral_token: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PerpUniverseItem {
    name: String,
    max_leverage: u64,
    #[serde(default)]
    only_isolated: bool,
    margin_mode: Option<MarginMode>,
    sz_decimals: i64,
    // margin_table_id: u64,
}

#[derive(Debug, Copy, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarginMode {
    StrictIsolated,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotTokens {
    universe: Vec<SpotUniverseItem>,
    tokens: Vec<Token>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotUniverseItem {
    // base and quote
    tokens: [u32; 2],
    name: String,
    index: usize,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Token {
    name: String,
    index: usize,
    token_id: B128,
    sz_decimals: i64,
    wei_decimals: i64,
    evm_contract: Option<EvmContract>,
}

impl From<Token> for SpotToken {
    fn from(token: Token) -> Self {
        let (evm_contract, cross_chain_address, evm_extra_decimals) =
            if let Some(contract) = token.evm_contract {
                (
                    Some(if token.name == "USDC" {
                        // map it to the contract in EVM
                        USDC_CONTRACT_IN_EVM
                    } else {
                        contract.address
                    }),
                    Some(generate_evm_transfer_address(token.index)),
                    contract.evm_extra_wei_decimals,
                )
            } else if token.name == "HYPE" {
                // map it to WHYPE
                (
                    Some(Address::repeat_byte(85)),
                    Some(Address::repeat_byte(34)),
                    10,
                )
            } else {
                (None, None, 0)
            };

        Self {
            name: token.name.clone(),
            token_id: token.token_id,
            index: token.index as u32,
            evm_contract,
            evm_extra_decimals,
            wei_decimals: token.wei_decimals,
            cross_chain_address: if token.name == "HYPE" {
                Some(Address::repeat_byte(34))
            } else {
                cross_chain_address
            },
            sz_decimals: token.sz_decimals,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct EvmContract {
    address: Address,
    evm_extra_wei_decimals: i64,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alloy::primitives::address;

    use super::*;
    use crate::hypercore;

    #[tokio::test]
    async fn test_spot_markets() {
        let client = reqwest::Client::new();
        let markets = spot_markets("https://api.hyperliquid.xyz", client)
            .await
            .unwrap();
        assert!(!markets.is_empty());
    }

    #[tokio::test]
    async fn test_evm_send_addresses() {
        let expected_addresses = HashMap::from([
            // PURR
            (
                "PURR/USDC",
                address!("0x2000000000000000000000000000000000000001"),
            ),
            // HFUN
            ("@1", address!("0x2000000000000000000000000000000000000002")),
            // USDT0
            (
                "@166",
                address!("0x200000000000000000000000000000000000010C"),
            ),
            // JEFF
            ("@4", address!("0x2000000000000000000000000000000000000005")),
            // HYPE
            (
                "@107",
                address!("0x2222222222222222222222222222222222222222"),
            ),
            // kHYPE
            (
                "@250",
                address!("0x2000000000000000000000000000000000000079"),
            ),
            // UBTC
            (
                "@142",
                address!("0x20000000000000000000000000000000000000c5"),
            ),
        ]);
        let spot = hypercore::spot_markets(mainnet_url(), reqwest::Client::new())
            .await
            .unwrap();
        for (key, value) in expected_addresses {
            let market = spot.iter().find(|market| market.name == key).unwrap();
            let address = market.tokens[0].cross_chain_address.unwrap();
            assert_eq!(address, value, "unexpected {address} <> {value}");
        }
    }

    #[tokio::test]
    async fn test_http_clearinghouse_state() {
        let client = hypercore::mainnet();
        // Use a known address with positions (Hyperliquid vault)
        let user = address!("0x162cc7c861ebd0c06b3d72319201150482518185");
        let state = client.clearinghouse_state(user).await.unwrap();

        // Verify structure is returned correctly
        assert!(state.time > 0);
        // Account should have some value
        assert!(state.margin_summary.account_value >= rust_decimal::Decimal::ZERO);
    }

    #[tokio::test]
    async fn test_http_user_balances() {
        let client = hypercore::mainnet();
        let user = address!("0xdfc24b077bc1425ad1dea75bcb6f8158e10df303");
        // Should return a list (possibly empty) without error
        let _balances = client.user_balances(user).await.unwrap();
    }

    #[tokio::test]
    async fn test_http_all_mids() {
        let client = hypercore::mainnet();
        let mids = client.all_mids().await.unwrap();

        // Should have prices for major markets
        assert!(mids.contains_key("BTC"));
        assert!(mids.contains_key("ETH"));
        assert!(*mids.get("BTC").unwrap() > rust_decimal::Decimal::ZERO);
    }

    #[tokio::test]
    async fn test_http_open_orders() {
        let client = hypercore::mainnet();
        let user = address!("0xdfc24b077bc1425ad1dea75bcb6f8158e10df303");
        // Should return a list (possibly empty) without error
        let _orders = client.open_orders(user).await.unwrap();
    }

    #[tokio::test]
    async fn test_http_perps() {
        let client = hypercore::mainnet();
        let perps = client.perps().await.unwrap();

        // Should have major markets
        assert!(!perps.is_empty());
        assert!(perps.iter().any(|m| m.name == "BTC"));
        assert!(perps.iter().any(|m| m.name == "ETH"));
    }

    #[tokio::test]
    async fn test_http_spot() {
        let client = hypercore::mainnet();
        let spots = client.spot().await.unwrap();

        // Should have spot markets
        assert!(!spots.is_empty());
    }
}
