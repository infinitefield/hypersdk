//! HyperCore type definitions for trading operations.
//!
//! This module contains all the core types used for interacting with the Hyperliquid
//! exchange API and WebSocket streams. It includes:
//!
//! # Core Components
//!
//! ## Trading Types
//! - [`Side`]: Buy or sell direction
//! - [`OrderType`]: Limit, market, or trigger orders
//! - [`TimeInForce`]: Order duration specifications (GTC, IOC, ALO)
//! - [`OrderStatus`]: Order lifecycle states
//! - [`OrderRequest`]: Order placement parameters
//! - [`BatchOrder`]: Batch order submission
//!
//! ## WebSocket Types
//! - [`Subscription`]: Subscribe to market data or user events
//! - [`Incoming`]: Messages received from the server
//! - [`Outgoing`]: Messages sent to the server
//! - [`Trade`]: Real-time trade events
//! - [`Fill`]: User order fills
//! - [`OrderUpdate`]: Order status changes
//! - [`L2Book`]: Order book snapshots and deltas
//! - [`Bbo`]: Best bid and offer updates
//!
//! ## Transfer Types
//! - [`UsdSend`]: Send USDC from perp balance
//! - [`SpotSend`]: Send spot tokens
//! - [`SendAsset`]: Send assets between accounts/DEXes
//!
//! ## API Response Types
//! - [`OrderResponseStatus`]: Result of order submission
//! - [`UserBalance`]: Account balance information
//!
//! # EIP-712 Signing
//!
//! All actions that modify state require EIP-712 signatures. Signing domains are
//! configured automatically by the SDK based on the chain and operation type.
//!
//! # Example: Placing an Order
//!
//! ```no_run
//! use hypersdk::hypercore::types::{
//!     OrderRequest, OrderTypePlacement, TimeInForce, Side
//! };
//!
//! // Example order structure - requires dec!() macro for prices/sizes
//! // let order = OrderRequest { ... };
//! ```
//!
//! # Example: WebSocket Subscription
//!
//! ```no_run
//! use hypersdk::hypercore::types::{Subscription, Outgoing};
//!
//! // Subscribe to BTC trades
//! let msg = Outgoing::Subscribe {
//!     subscription: Subscription::Trades {
//!         coin: "BTC".to_string()
//!     }
//! };
//! ```

use std::{
    collections::HashMap,
    fmt,
    hash::{Hash, Hasher},
    time::Duration,
};

use alloy::{
    dyn_abi::Eip712Domain,
    primitives::{Address, B128, U256},
    signers::k256::ecdsa::RecoveryId,
    sol_types::eip712_domain,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use crate::hypercore::{Chain, Cloid, OidOrCloid, SpotToken};

pub mod api;
pub(super) mod solidity;

// Re-export important raw types for convenience
pub use api::{Action, ActionRequest, MultiSigAction, MultiSigPayload};

// Import from raw module (which is now a submodule)
use api::{SendAssetAction, SpotSendAction, UsdSendAction};

/// Domain for Core mainnet EIP‑712 signing.
/// This domain is used when creating signatures for transactions on the mainnet.
pub(super) const CORE_MAINNET_EIP712_DOMAIN: Eip712Domain = eip712_domain! {
    name: "Exchange",
    version: "1",
    chain_id: 1337,
    verifying_contract: Address::ZERO,
};

/// Domain for Arbitrum mainnet EIP‑712 signing.
/// This domain is used when creating signatures for transactions on Arbitrum.
pub const ARBITRUM_MAINNET_EIP712_DOMAIN: Eip712Domain = eip712_domain! {
    name: "HyperliquidSignTransaction",
    version: "1",
    chain_id: 42161,
    verifying_contract: Address::ZERO,
};

/// Domain for L1 testnet EIP‑712 signing.
/// This domain is used when creating multisig signatures on testnet (chainId 0x66eee = 421614).
pub const ARBITRUM_TESTNET_EIP712_DOMAIN: Eip712Domain = eip712_domain! {
    name: "HyperliquidSignTransaction",
    version: "1",
    chain_id: 421614,
    verifying_contract: Address::ZERO,
};

/// HIP-3 exchange.
#[derive(Debug, Clone, derive_more::Display)]
#[display("{name}")]
pub struct Dex {
    pub(super) name: String,
    pub(super) index: usize,
}

impl Dex {
    /// Creates a new `Dex` instance.
    ///
    /// # Parameters
    ///
    /// - `name`: The name of the DEX.
    /// - `index`: The numerical index associated with the DEX.
    ///
    /// # Returns
    ///
    /// A new `Dex` instance.
    pub fn new(name: String, index: usize) -> Dex {
        Dex { name, index }
    }

    /// Returns the DEX name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl PartialEq for Dex {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Dex {}

impl Hash for Dex {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

/// Side for a trade or an order.
///
/// `Bid` represents a buy order, `Ask` represents a sell order.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, derive_more::Display,
)]
pub enum Side {
    #[serde(rename = "B")]
    Bid,
    #[serde(rename = "A")]
    Ask,
}

/// WebSocket outgoing message.
///
/// This enum represents messages sent from the client to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
#[serde(rename_all = "camelCase")]
pub enum Outgoing {
    Subscribe { subscription: Subscription },
    Unsubscribe { subscription: Subscription },
    Ping,
    Pong,
}

/// WebSocket subscription request.
///
/// Each variant corresponds to a subscription type that can be requested from the WebSocket API.
/// After subscribing, you'll receive corresponding [`Incoming`] messages.
///
/// # Market Data Subscriptions
///
/// | Subscription | Incoming Message | Description |
/// |--------------|------------------|-------------|
/// | [`Bbo`](Self::Bbo) | [`Incoming::Bbo`] | Best bid and offer updates |
/// | [`Trades`](Self::Trades) | [`Incoming::Trades`] | Real-time trades |
/// | [`L2Book`](Self::L2Book) | [`Incoming::L2Book`] | Order book updates |
/// | [`Candle`](Self::Candle) | [`Incoming::Candle`] | Candlestick (OHLCV) data |
/// | [`AllMids`](Self::AllMids) | [`Incoming::AllMids`] | Mid prices for all markets |
///
/// # User-Specific Subscriptions
///
/// | Subscription | Incoming Message | Description |
/// |--------------|------------------|-------------|
/// | [`OrderUpdates`](Self::OrderUpdates) | [`Incoming::OrderUpdates`] | Order status changes |
/// | [`UserFills`](Self::UserFills) | [`Incoming::UserFills`] | Trade fills |
///
/// # Related Types
///
/// - [`Incoming`]: Messages received from WebSocket subscriptions
/// - [`WebSocket`](crate::hypercore::ws::Connection): WebSocket client
/// - [`Bbo`], [`Trade`], [`L2Book`], [`Candle`], [`OrderUpdate`], [`Fill`]: Data types
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore::{self, types::*};
/// use futures::StreamExt;
///
/// # async fn example() {
/// let mut ws = hypercore::mainnet_ws();
///
/// // Subscribe to market data
/// ws.subscribe(Subscription::Bbo { coin: "BTC".into() });
/// ws.subscribe(Subscription::Trades { coin: "ETH".into() });
/// ws.subscribe(Subscription::Candle {
///     coin: "BTC".into(),
///     interval: "15m".into()
/// });
///
/// // Subscribe to user events
/// let user = "0x...".parse().unwrap();
/// ws.subscribe(Subscription::OrderUpdates { user });
/// ws.subscribe(Subscription::UserFills { user });
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize, derive_more::Display)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Subscription {
    /// Best bid and offer updates
    #[display("bbo({coin})")]
    Bbo { coin: String },
    /// Real-time trade feed
    #[display("trades({coin})")]
    Trades { coin: String },
    /// Order book snapshots and updates
    #[display("l2Book({coin})")]
    L2Book { coin: String },
    /// Real-time candlestick updates
    #[display("candle({coin}@{interval})")]
    Candle { coin: String, interval: String },
    /// Mid prices for all markets
    #[display("allMids({dex:?})")]
    AllMids {
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    /// Order status updates for user
    #[display("orderUpdates({user})")]
    OrderUpdates { user: Address },
    /// Fill events for user
    #[display("userFills({user})")]
    UserFills { user: Address },
}

/// Hyperliquid websocket message.
///
/// This enum represents all message types received from the WebSocket server.
/// Messages arrive in response to subscriptions or as confirmation messages.
///
/// # Message Types
///
/// - **SubscriptionResponse**: Confirmation of subscription/unsubscription
/// - **Bbo**: Best bid and offer update
/// - **L2Book**: Order book snapshot or delta
/// - **Candle**: Candlestick (OHLCV) update
/// - **AllMids**: Mid prices for all markets
/// - **Trades**: Trade events for a market
/// - **OrderUpdates**: Order status changes for a user
/// - **UserFills**: Fill events for a user
/// - **Ping/Pong**: Heartbeat messages
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::Incoming;
///
/// // Match on incoming messages
/// # fn handle_message(msg: Incoming) {
/// match msg {
///     Incoming::Trades(trades) => {
///         for trade in trades {
///             println!("Trade: {} @ {}", trade.sz, trade.px);
///         }
///     }
///     Incoming::Candle(candle) => {
///         println!("Candle: O:{} H:{} L:{} C:{}",
///             candle.open, candle.high, candle.low, candle.close);
///     }
///     Incoming::OrderUpdates(updates) => {
///         for update in updates {
///             println!("Order {}: {:?}", update.order.oid, update.status);
///         }
///     }
///     Incoming::Ping => {
///         // Server sent ping, reply with pong
///     }
///     _ => {}
/// }
/// # }
/// ```
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "channel", content = "data")]
pub enum Incoming {
    /// Confirmation of subscription/unsubscription
    SubscriptionResponse(Outgoing),
    /// Best bid and offer update
    Bbo(Bbo),
    /// Order book snapshot or delta
    L2Book(L2Book),
    /// Candlestick update
    Candle(Candle),
    /// Mid prices for all markets
    AllMids {
        dex: Option<String>,
        mids: HashMap<String, Decimal>,
    },
    /// Trade events for a market
    Trades(Vec<Trade>),
    /// Order status changes for a user
    OrderUpdates(Vec<OrderUpdate>),
    /// Fill events for a user
    UserFills { user: Address, fills: Vec<Fill> },
    /// Server heartbeat ping
    Ping,
    /// Server heartbeat pong
    Pong,
}

/// WebSocket order update.
///
/// Contains status, timestamp, and the original order details.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderUpdate {
    pub status: OrderStatus,
    pub status_timestamp: u64,
    pub order: WsBasicOrder,
}

/// Best bid offer.
///
/// Provides the best bid and ask for a coin at a specific time.
///
/// # Fields
///
/// - `coin`: Market symbol (e.g., "BTC", "ETH")
/// - `time`: Timestamp in milliseconds
/// - `bbo`: Tuple of (best_bid, best_ask), either may be None if no liquidity
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::Bbo;
///
/// # fn process_bbo(bbo: Bbo) {
/// // Access best bid and ask
/// if let Some(bid) = bbo.bid() {
///     println!("Best bid: {} @ {}", bid.sz, bid.px);
/// }
/// if let Some(ask) = bbo.ask() {
///     println!("Best ask: {} @ {}", ask.sz, ask.px);
/// }
///
/// // Calculate spread
/// if let Some(spread) = bbo.spread() {
///     println!("Spread: {}", spread);
/// }
/// # }
/// ```
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Bbo {
    /// Market symbol
    pub coin: String,
    /// Timestamp in milliseconds
    pub time: u64,
    /// (best_bid, best_ask)
    pub bbo: (Option<BookLevel>, Option<BookLevel>),
}

impl Bbo {
    /// Returns the best bid level, if available.
    #[must_use]
    pub fn bid(&self) -> Option<&BookLevel> {
        self.bbo.0.as_ref()
    }

    /// Returns the best ask level, if available.
    #[must_use]
    pub fn ask(&self) -> Option<&BookLevel> {
        self.bbo.1.as_ref()
    }

    /// Returns the mid price (average of bid and ask), if both are available.
    #[must_use]
    pub fn mid(&self) -> Option<Decimal> {
        let bid = self.bid()?;
        let ask = self.ask()?;
        Some((bid.px + ask.px) / rust_decimal::Decimal::TWO)
    }

    /// Returns the spread (ask - bid), if both are available.
    #[must_use]
    pub fn spread(&self) -> Option<Decimal> {
        let bid = self.bid()?;
        let ask = self.ask()?;
        Some(ask.px - bid.px)
    }
}

/// WebSocket book level.
///
/// Represents a single price level on the order book.
///
/// # Fields
///
/// - `px`: Price level
/// - `sz`: Total size at this level
/// - `n`: Number of orders at this level
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::BookLevel;
/// use rust_decimal::dec;
///
/// let level = BookLevel {
///     px: dec!(50000),  // $50k
///     sz: dec!(2.5),    // 2.5 BTC
///     n: 3,             // 3 orders
/// };
/// ```
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BookLevel {
    /// Price level
    pub px: Decimal,
    /// Total size at this level
    pub sz: Decimal,
    /// Number of orders at this level
    pub n: usize,
}

/// WebSocket trade.
///
/// Describes a single trade that occurred on the exchange.
///
/// # Fields
///
/// - `coin`: Market symbol (e.g., "BTC", "ETH")
/// - `side`: Direction of the trade from the taker's perspective (Bid = buy, Ask = sell)
/// - `px`: Execution price
/// - `sz`: Trade size
/// - `time`: Timestamp in milliseconds
/// - `hash`: Transaction hash
/// - `tid`: Trade ID (monotonically increasing)
/// - `liquidation`: Optional liquidation details if this was a liquidation
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::{Trade, Side};
/// use rust_decimal::dec;
///
/// # fn process_trade(trade: Trade) {
/// // Check trade direction
/// match trade.side {
///     Side::Bid => println!("Buy trade: {} @ {}", trade.sz, trade.px),
///     Side::Ask => println!("Sell trade: {} @ {}", trade.sz, trade.px),
/// }
///
/// // Calculate notional value
/// let notional = trade.notional();
/// println!("Trade value: ${}", notional);
///
/// // Check if liquidation
/// if trade.is_liquidation() {
///     println!("This was a liquidation trade");
/// }
/// # }
/// ```
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Trade {
    /// Market symbol
    pub coin: String,
    /// Taker's side (Bid = buy, Ask = sell)
    pub side: Side,
    /// Execution price
    pub px: Decimal,
    /// Trade size
    pub sz: Decimal,
    /// Timestamp in milliseconds
    pub time: u64,
    /// Transaction hash
    pub hash: String,
    /// Trade ID
    pub tid: u64,
    /// Liquidation details, if applicable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidation: Option<Liquidation>,
}

impl Trade {
    /// Returns the notional value of the trade (price * size).
    #[must_use]
    pub fn notional(&self) -> Decimal {
        self.px * self.sz
    }

    /// Returns true if this trade was a liquidation.
    #[must_use]
    pub fn is_liquidation(&self) -> bool {
        self.liquidation.is_some()
    }

    /// Returns true if this trade was a buy (from taker's perspective).
    #[must_use]
    pub fn is_buy(&self) -> bool {
        matches!(self.side, Side::Bid)
    }

    /// Returns true if this trade was a sell (from taker's perspective).
    #[must_use]
    pub fn is_sell(&self) -> bool {
        matches!(self.side, Side::Ask)
    }
}

/// Candle interval for historical data.
///
/// Specifies the time period covered by each candle.
///
/// # Available Intervals
///
/// - Minutes: `OneMinute`, `ThreeMinutes`, `FiveMinutes`, `FifteenMinutes`, `ThirtyMinutes`
/// - Hours: `OneHour`, `TwoHours`, `FourHours`, `EightHours`, `TwelveHours`
/// - Days and above: `OneDay`, `ThreeDays`, `OneWeek`, `OneMonth`
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::CandleInterval;
///
/// let interval = CandleInterval::FifteenMinutes;
/// assert_eq!(interval.to_string(), "15m");
///
/// let parsed: CandleInterval = "15m".parse().unwrap();
/// assert_eq!(parsed, CandleInterval::FifteenMinutes);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display)]
pub enum CandleInterval {
    #[serde(rename = "1m")]
    #[display("1m")]
    OneMinute,
    #[serde(rename = "3m")]
    #[display("3m")]
    ThreeMinutes,
    #[serde(rename = "5m")]
    #[display("5m")]
    FiveMinutes,
    #[serde(rename = "15m")]
    #[display("15m")]
    FifteenMinutes,
    #[serde(rename = "30m")]
    #[display("30m")]
    ThirtyMinutes,
    #[serde(rename = "1h")]
    #[display("1h")]
    OneHour,
    #[serde(rename = "2h")]
    #[display("2h")]
    TwoHours,
    #[serde(rename = "4h")]
    #[display("4h")]
    FourHours,
    #[serde(rename = "8h")]
    #[display("8h")]
    EightHours,
    #[serde(rename = "12h")]
    #[display("12h")]
    TwelveHours,
    #[serde(rename = "1d")]
    #[display("1d")]
    OneDay,
    #[serde(rename = "3d")]
    #[display("3d")]
    ThreeDays,
    #[serde(rename = "1w")]
    #[display("1w")]
    OneWeek,
    #[serde(rename = "1M")]
    #[display("1M")]
    OneMonth,
}

impl CandleInterval {
    /// Returns the duration represented by this candle interval.
    ///
    /// ## Notes
    ///
    /// - For all fixed intervals (minutes, hours, days, weeks), the duration
    ///   is strictly defined.
    /// - For `OneMonth`, this method assumes **30 days** by default.
    ///
    /// If you need a calendar-aware duration (e.g. 28/29/30/31 days),
    /// use [`to_duration_with_month_days`] instead.
    pub fn to_duration(&self) -> Duration {
        self.to_duration_with_month_days(30)
    }

    /// Returns the duration represented by this candle interval, using the
    /// provided number of days for a calendar month.
    ///
    /// ## Parameters
    ///
    /// - `month_days`: Number of days in the month (e.g. 28, 29, 30, or 31).
    ///
    /// ## Notes
    ///
    /// - This parameter is **only meaningful** for `OneMonth`.
    /// - For all other intervals, the value of `month_days` is ignored.
    ///
    /// ## When to use
    ///
    /// Use this method when:
    /// - Replaying historical data
    /// - Performing backtests
    /// - Working with calendar-aware candle alignment
    ///
    /// ## Example
    ///
    /// ```rust
    /// let interval = CandleInterval::OneMonth;
    ///
    /// // February
    /// let feb = interval.to_duration_with_month_days(28);
    ///
    /// // March
    /// let mar = interval.to_duration_with_month_days(31);
    /// ```
    pub fn to_duration_with_month_days(&self, month_days: u32) -> Duration {
        match self {
            CandleInterval::OneMinute => Duration::from_secs(60),
            CandleInterval::ThreeMinutes => Duration::from_secs(3 * 60),
            CandleInterval::FiveMinutes => Duration::from_secs(5 * 60),
            CandleInterval::FifteenMinutes => Duration::from_secs(15 * 60),
            CandleInterval::ThirtyMinutes => Duration::from_secs(30 * 60),

            CandleInterval::OneHour => Duration::from_secs(60 * 60),
            CandleInterval::TwoHours => Duration::from_secs(2 * 60 * 60),
            CandleInterval::FourHours => Duration::from_secs(4 * 60 * 60),
            CandleInterval::EightHours => Duration::from_secs(8 * 60 * 60),
            CandleInterval::TwelveHours => Duration::from_secs(12 * 60 * 60),

            CandleInterval::OneDay => Duration::from_secs(24 * 60 * 60),
            CandleInterval::ThreeDays => Duration::from_secs(3 * 24 * 60 * 60),
            CandleInterval::OneWeek => Duration::from_secs(7 * 24 * 60 * 60),

            CandleInterval::OneMonth => Duration::from_secs(month_days as u64 * 24 * 60 * 60),
        }
    }
}

impl std::str::FromStr for CandleInterval {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1m" => Ok(Self::OneMinute),
            "3m" => Ok(Self::ThreeMinutes),
            "5m" => Ok(Self::FiveMinutes),
            "15m" => Ok(Self::FifteenMinutes),
            "30m" => Ok(Self::ThirtyMinutes),
            "1h" => Ok(Self::OneHour),
            "2h" => Ok(Self::TwoHours),
            "4h" => Ok(Self::FourHours),
            "8h" => Ok(Self::EightHours),
            "12h" => Ok(Self::TwelveHours),
            "1d" => Ok(Self::OneDay),
            "3d" => Ok(Self::ThreeDays),
            "1w" => Ok(Self::OneWeek),
            "1M" => Ok(Self::OneMonth),
            _ => anyhow::bail!("Invalid candle interval: {}", s),
        }
    }
}

/// WebSocket candle (OHLCV bar).
///
/// Represents a single candlestick with open, high, low, close prices and volume.
///
/// # Fields
///
/// - `open_time`: Candle open time in milliseconds
/// - `close_time`: Candle close time in milliseconds
/// - `coin`: Market symbol (e.g., "BTC", "ETH")
/// - `interval`: Candle interval (e.g., "15m", "1h", "1d")
/// - `open`: Open price (first trade in the period)
/// - `high`: High price (highest trade in the period)
/// - `low`: Low price (lowest trade in the period)
/// - `close`: Close price (last trade in the period)
/// - `volume`: Volume (total traded amount in the period)
/// - `num_trades`: Number of trades in this candle
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Candle {
    /// Candle open time (milliseconds)
    #[serde(rename = "t")]
    pub open_time: u64,
    /// Candle close time (milliseconds)
    #[serde(rename = "T")]
    pub close_time: u64,
    /// Market symbol
    #[serde(rename = "s")]
    pub coin: String,
    /// Interval
    #[serde(rename = "i")]
    pub interval: String,
    /// Open price
    #[serde(rename = "o")]
    pub open: Decimal,
    /// High price
    #[serde(rename = "h")]
    pub high: Decimal,
    /// Low price
    #[serde(rename = "l")]
    pub low: Decimal,
    /// Close price
    #[serde(rename = "c")]
    pub close: Decimal,
    /// Volume
    #[serde(rename = "v")]
    pub volume: Decimal,
    /// Number of trades
    #[serde(rename = "n")]
    pub num_trades: u64,
}

/// WebSocket L2Book.
///
/// Contains the order book snapshot or deltas for a coin.
///
/// # Fields
///
/// - `coin`: Market symbol (e.g., "BTC", "ETH")
/// - `time`: Timestamp in milliseconds
/// - `snapshot`: True if this is a full snapshot, false/None if it's a delta update
/// - `levels`: Array of [bids, asks], each containing sorted price levels
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::L2Book;
///
/// # fn process_book(book: L2Book) {
/// // Check if this is a snapshot or delta
/// if book.is_snapshot() {
///     println!("Received full book snapshot");
/// } else {
///     println!("Received book delta update");
/// }
///
/// // Access bids and asks
/// for bid in book.bids() {
///     println!("Bid: {} @ {}", bid.sz, bid.px);
/// }
/// for ask in book.asks() {
///     println!("Ask: {} @ {}", ask.sz, ask.px);
/// }
///
/// // Get best bid and ask
/// if let Some(best_bid) = book.best_bid() {
///     println!("Best bid: {}", best_bid.px);
/// }
/// if let Some(best_ask) = book.best_ask() {
///     println!("Best ask: {}", best_ask.px);
/// }
/// # }
/// ```
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct L2Book {
    /// Market symbol
    pub coin: String,
    /// Timestamp in milliseconds
    pub time: u64,
    /// True if snapshot, false/None if delta
    #[serde(default)]
    pub snapshot: Option<bool>,
    /// [bids, asks]
    pub levels: [Vec<BookLevel>; 2],
}

impl L2Book {
    /// Returns true if this is a full snapshot (not a delta update).
    #[must_use]
    pub fn is_snapshot(&self) -> bool {
        self.snapshot.unwrap_or(false)
    }

    /// Returns the bid levels (sorted from highest to lowest).
    #[must_use]
    pub fn bids(&self) -> &[BookLevel] {
        &self.levels[0]
    }

    /// Returns the ask levels (sorted from lowest to highest).
    #[must_use]
    pub fn asks(&self) -> &[BookLevel] {
        &self.levels[1]
    }

    /// Returns the best bid level, if available.
    #[must_use]
    pub fn best_bid(&self) -> Option<&BookLevel> {
        self.bids().first()
    }

    /// Returns the best ask level, if available.
    #[must_use]
    pub fn best_ask(&self) -> Option<&BookLevel> {
        self.asks().first()
    }

    /// Returns the mid price (average of best bid and ask), if both are available.
    #[must_use]
    pub fn mid(&self) -> Option<Decimal> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        Some((bid.px + ask.px) / rust_decimal::Decimal::TWO)
    }

    /// Returns the spread (best ask - best bid), if both are available.
    #[must_use]
    pub fn spread(&self) -> Option<Decimal> {
        let bid = self.best_bid()?;
        let ask = self.best_ask()?;
        Some(ask.px - bid.px)
    }
}

/// WebSocket fill.
///
/// Describes a filled order for a user. Contains execution details and position impact.
///
/// # Fields
///
/// - `coin`: Market symbol
/// - `px`: Fill price
/// - `sz`: Fill size
/// - `side`: Order side (Bid = buy, Ask = sell)
/// - `time`: Timestamp in milliseconds
/// - `start_position`: Position size before this fill
/// - `dir`: Direction ("Open Long", "Close Long", "Open Short", "Close Short")
/// - `closed_pnl`: Realized PnL from closing position (0 if opening)
/// - `hash`: Transaction hash
/// - `oid`: Order ID
/// - `crossed`: True if this fill crossed the spread (taker)
/// - `fee`: Fee amount
/// - `tid`: Trade ID
/// - `cloid`: Optional client order ID
/// - `fee_token`: Token used for fee payment
/// - `liquidation`: Optional liquidation details
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::Fill;
/// use rust_decimal::Decimal;
///
/// # fn process_fill(fill: Fill) {
/// // Check if this opened or closed a position
/// if fill.is_opening() {
///     println!("Opened position: {} @ {}", fill.sz, fill.px);
/// } else {
///     println!("Closed position: {} @ {} (PnL: {})", fill.sz, fill.px, fill.closed_pnl);
/// }
///
/// // Calculate notional value
/// let notional = fill.notional();
/// println!("Fill value: ${}", notional);
///
/// // Check if maker or taker
/// if fill.is_maker() {
///     println!("Maker fill (added liquidity)");
/// } else {
///     println!("Taker fill (took liquidity)");
/// }
/// # }
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fill {
    /// Market symbol
    pub coin: String,
    /// Fill price
    pub px: Decimal,
    /// Fill size
    pub sz: Decimal,
    /// Order side
    pub side: Side,
    /// Timestamp in milliseconds
    pub time: u64,
    /// Position before fill
    pub start_position: Decimal,
    /// Direction (Open/Close Long/Short)
    pub dir: String,
    /// Realized PnL from closing
    pub closed_pnl: Decimal,
    /// Transaction hash
    pub hash: String,
    /// Order ID
    pub oid: u64,
    /// True if taker (crossed spread)
    pub crossed: bool,
    /// Fee amount
    pub fee: Decimal,
    /// Trade ID
    pub tid: u64,
    /// Client order ID
    pub cloid: Option<B128>,
    /// Fee token
    pub fee_token: String,
    /// Liquidation details, if applicable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidation: Option<Liquidation>,
}

impl Fill {
    /// Returns the notional value of the fill (price * size).
    #[must_use]
    pub fn notional(&self) -> Decimal {
        self.px * self.sz
    }

    /// Returns true if this fill opened a position (closed_pnl is zero).
    #[must_use]
    pub fn is_opening(&self) -> bool {
        self.closed_pnl.is_zero()
    }

    /// Returns true if this fill closed a position (closed_pnl is non-zero).
    #[must_use]
    pub fn is_closing(&self) -> bool {
        !self.closed_pnl.is_zero()
    }

    /// Returns true if this was a maker fill (added liquidity).
    #[must_use]
    pub fn is_maker(&self) -> bool {
        !self.crossed
    }

    /// Returns true if this was a taker fill (took liquidity).
    #[must_use]
    pub fn is_taker(&self) -> bool {
        self.crossed
    }

    /// Returns true if this fill was a liquidation.
    #[must_use]
    pub fn is_liquidation(&self) -> bool {
        self.liquidation.is_some()
    }

    /// Returns the net proceeds after fees (notional - fee).
    #[must_use]
    pub fn net_proceeds(&self) -> Decimal {
        self.notional() - self.fee
    }
}

/// Order details.
///
/// Basic information needed for creating or updating an order.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde_as]
#[serde(rename_all = "camelCase")]
pub struct BasicOrder {
    pub timestamp: u64,
    pub coin: String,
    pub side: Side,
    pub limit_px: Decimal,
    pub sz: Decimal,
    pub oid: u64,
    pub orig_sz: Decimal,
    pub cloid: Option<B128>,
    pub order_type: OrderType,
    pub tif: Option<TimeInForce>,
    pub reduce_only: bool,
}

/// Basic order information for WebSocket updates.
///
/// This struct represents core details of an order, typically seen in WebSocket
/// messages like [`OrderUpdate`]. It is a simplified version of [`BasicOrder`],
/// omitting placement-specific fields such as `order_type`, `tif`, and `reduce_only`,
/// as these are not relevant for tracking existing order state via WebSockets.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde_as]
#[serde(rename_all = "camelCase")]
pub struct WsBasicOrder {
    pub timestamp: u64,
    pub coin: String,
    pub side: Side,
    pub limit_px: Decimal,
    pub sz: Decimal,
    pub oid: u64,
    pub orig_sz: Decimal,
    pub cloid: Option<B128>,
}

/// Liquidation details.
///
/// Information about a liquidation event associated with a trade or fill.
///
/// # Fields
///
/// - `liquidated_user`: Address of the user being liquidated
/// - `mark_px`: Mark price at liquidation
/// - `method`: Liquidation method used
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Liquidation {
    /// Address of liquidated user
    pub liquidated_user: String,
    /// Mark price at liquidation
    pub mark_px: Decimal,
    /// Liquidation method
    pub method: String,
}

/// Order type.
///
/// Determines the behaviour of the order (limit, market, or trigger).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub enum OrderType {
    Limit,
    Market,
    Trigger,
}

/// Time‑in‑force.
///
/// Specifies how long an order remains active and how it interacts with the order book.
///
/// # Variants
///
/// - **Alo** (Add Liquidity Only): Order will only be placed if it adds liquidity to the book.
///   If it would take liquidity (match immediately), it's rejected. This is a maker-only order.
///
/// - **Ioc** (Immediate or Cancel): Order executes immediately against available liquidity,
///   and any unfilled portion is cancelled. This is a taker-only order that never rests on the book.
///
/// - **Gtc** (Good Till Cancel): Order remains active until fully filled or explicitly cancelled.
///   This is the standard order type that can both take and make liquidity.
///
/// - **FrontendMarket**: Special order type used by the Hyperliquid frontend for market orders.
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::TimeInForce;
///
/// // Maker order: only adds liquidity, never takes
/// let maker_tif = TimeInForce::Alo;
///
/// // Taker order: executes immediately or cancels
/// let taker_tif = TimeInForce::Ioc;
///
/// // Standard order: remains active until filled or cancelled
/// let standard_tif = TimeInForce::Gtc;
/// ```
#[derive(Debug, Deserialize, Clone, Copy, Serialize)]
#[serde(rename = "PascalCase")]
pub enum TimeInForce {
    /// Add Liquidity Only - maker-only order
    Alo,
    /// Immediate or Cancel - taker-only order
    Ioc,
    /// Good Till Cancel - standard order
    Gtc,
    /// Frontend market order type
    FrontendMarket,
}

/// Order status.
///
/// Represents the lifecycle state of an order. Orders can be in active states (Open, Triggered)
/// or terminal states (Filled, Canceled, Rejected).
///
/// # Active States
///
/// - **Open**: Order is active on the book awaiting execution
/// - **Triggered**: Trigger order has been activated and is now being placed
///
/// # Success States
///
/// - **Filled**: Order was completely filled
///
/// # Cancellation States
///
/// Orders can be cancelled for various reasons:
///
/// - **Canceled**: User-requested cancellation
/// - **MarginCanceled**: Cancelled due to insufficient margin
/// - **VaultWithdrawalCanceled**: Cancelled due to vault withdrawal
/// - **OpenInterestCapCanceled**: Cancelled due to open interest cap
/// - **SelfTradeCanceled**: Cancelled to prevent self-trading
/// - **ReduceOnlyCanceled**: Reduce-only order would have increased position
/// - **SiblingFilledCanceled**: Associated order was filled (e.g., TP/SL pair)
/// - **DelistedCanceled**: Market was delisted
/// - **LiquidatedCanceled**: Position was liquidated
/// - **ScheduledCancel**: User-scheduled cancellation executed
/// - **IocCancelRejected**: IOC order had unfilled portion
///
/// # Rejection States
///
/// Orders can be rejected before placement:
///
/// - **Rejected**: Generic rejection
/// - **TickRejected**: Price doesn't match tick size
/// - **MinTradeNtlRejected**: Order value below minimum notional
/// - **PerpMarginRejected**: Insufficient margin for perp order
/// - **ReduceOnlyRejected**: Reduce-only order would increase position
/// - **BadAloPxRejected**: ALO order price would take liquidity
/// - **BadTriggerPxRejected**: Invalid trigger price
/// - **MarketOrderNoLiquidityRejected**: No liquidity for market order
/// - **PositionIncreaseAtOpenInterestCapRejected**: Would exceed open interest cap
/// - **PositionFlipAtOpenInterestCapRejected**: Would flip position at cap
/// - **TooAggressiveAtOpenInterestCapRejected**: Too aggressive near cap
/// - **OpenInterestIncreaseRejected**: Would increase open interest past limit
/// - **InsufficientSpotBalanceRejected**: Insufficient spot balance
/// - **OracleRejected**: Oracle price check failed
/// - **PerpMaxPositionRejected**: Would exceed max position size
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::OrderStatus;
///
/// let status = OrderStatus::Filled;
/// assert!(status.is_finished());
///
/// let status = OrderStatus::Open;
/// assert!(!status.is_finished());
/// ```
#[derive(Debug, Copy, Clone, Hash, Deserialize, Serialize, derive_more::Display)]
#[serde(rename_all = "camelCase")]
pub enum OrderStatus {
    /// Order is active on the book
    Open,
    /// Order was completely filled
    Filled,
    /// User-requested cancellation
    Canceled,
    /// Trigger order activated
    Triggered,
    /// Generic rejection
    Rejected,
    /// Cancelled due to insufficient margin
    MarginCanceled,
    /// Cancelled due to vault withdrawal
    VaultWithdrawalCanceled,
    /// Cancelled due to open interest cap
    OpenInterestCapCanceled,
    /// Cancelled to prevent self-trading
    SelfTradeCanceled,
    /// Reduce-only order would increase position
    ReduceOnlyCanceled,
    /// Associated order was filled
    SiblingFilledCanceled,
    /// Market was delisted
    DelistedCanceled,
    /// Position was liquidated
    LiquidatedCanceled,
    /// User-scheduled cancellation
    ScheduledCancel,
    /// Price doesn't match tick size
    TickRejected,
    /// Order value below minimum
    MinTradeNtlRejected,
    /// Insufficient margin for perp
    PerpMarginRejected,
    /// Reduce-only would increase position
    ReduceOnlyRejected,
    /// ALO price would take liquidity
    BadAloPxRejected,
    /// IOC unfilled portion cancelled
    IocCancelRejected,
    /// Invalid trigger price
    BadTriggerPxRejected,
    /// No liquidity for market order
    MarketOrderNoLiquidityRejected,
    /// Would exceed open interest cap
    PositionIncreaseAtOpenInterestCapRejected,
    /// Would flip position at cap
    PositionFlipAtOpenInterestCapRejected,
    /// Too aggressive near cap
    TooAggressiveAtOpenInterestCapRejected,
    /// Would exceed open interest limit
    OpenInterestIncreaseRejected,
    /// Insufficient spot balance
    InsufficientSpotBalanceRejected,
    /// Oracle check failed
    OracleRejected,
    /// Would exceed max position
    PerpMaxPositionRejected,
}

impl OrderStatus {
    /// Returns whether the order is finished (not Open).
    ///
    /// # Example
    ///
    /// ```rust
    /// use hypersdk::hypercore::types::OrderStatus;
    ///
    /// assert!(OrderStatus::Filled.is_finished());
    /// assert!(OrderStatus::Canceled.is_finished());
    /// assert!(!OrderStatus::Open.is_finished());
    /// ```
    #[must_use]
    pub fn is_finished(&self) -> bool {
        !matches!(self, OrderStatus::Open)
    }

    /// Returns whether the order was successfully filled.
    ///
    /// # Example
    ///
    /// ```rust
    /// use hypersdk::hypercore::types::OrderStatus;
    ///
    /// assert!(OrderStatus::Filled.is_filled());
    /// assert!(!OrderStatus::Canceled.is_filled());
    /// ```
    #[must_use]
    pub fn is_filled(&self) -> bool {
        matches!(self, OrderStatus::Filled)
    }

    /// Returns whether the order was cancelled (any cancellation reason).
    ///
    /// # Example
    ///
    /// ```rust
    /// use hypersdk::hypercore::types::OrderStatus;
    ///
    /// assert!(OrderStatus::Canceled.is_cancelled());
    /// assert!(OrderStatus::MarginCanceled.is_cancelled());
    /// assert!(!OrderStatus::Filled.is_cancelled());
    /// ```
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(
            self,
            OrderStatus::Canceled
                | OrderStatus::MarginCanceled
                | OrderStatus::VaultWithdrawalCanceled
                | OrderStatus::OpenInterestCapCanceled
                | OrderStatus::SelfTradeCanceled
                | OrderStatus::ReduceOnlyCanceled
                | OrderStatus::SiblingFilledCanceled
                | OrderStatus::DelistedCanceled
                | OrderStatus::LiquidatedCanceled
                | OrderStatus::ScheduledCancel
                | OrderStatus::IocCancelRejected
        )
    }

    /// Returns whether the order was rejected (any rejection reason).
    ///
    /// # Example
    ///
    /// ```rust
    /// use hypersdk::hypercore::types::OrderStatus;
    ///
    /// assert!(OrderStatus::TickRejected.is_rejected());
    /// assert!(OrderStatus::PerpMarginRejected.is_rejected());
    /// assert!(!OrderStatus::Filled.is_rejected());
    /// ```
    #[must_use]
    pub fn is_rejected(&self) -> bool {
        matches!(
            self,
            OrderStatus::Rejected
                | OrderStatus::TickRejected
                | OrderStatus::MinTradeNtlRejected
                | OrderStatus::PerpMarginRejected
                | OrderStatus::ReduceOnlyRejected
                | OrderStatus::BadAloPxRejected
                | OrderStatus::BadTriggerPxRejected
                | OrderStatus::MarketOrderNoLiquidityRejected
                | OrderStatus::PositionIncreaseAtOpenInterestCapRejected
                | OrderStatus::PositionFlipAtOpenInterestCapRejected
                | OrderStatus::TooAggressiveAtOpenInterestCapRejected
                | OrderStatus::OpenInterestIncreaseRejected
                | OrderStatus::InsufficientSpotBalanceRejected
                | OrderStatus::OracleRejected
                | OrderStatus::PerpMaxPositionRejected
        )
    }
}

/// Send USDC from the perpetual balance (inner data).
///
/// This is the core data structure for a USDC transfer. To create a signable action,
/// use the `into_action()` method to convert it to a `UsdSendAction`.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#core-usdc-transfer>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsdSend {
    pub destination: Address,
    pub amount: Decimal,
    pub time: u64,
}

impl UsdSend {
    /// Converts this into a signable `UsdSendAction`.
    ///
    /// # Parameters
    ///
    /// - `signature_chain_id`: The chain ID for signature verification (e.g., [`super::ARBITRUM_MAINNET_CHAIN_ID`])
    /// - `chain`: Whether this is mainnet or testnet
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let send = UsdSend {
    ///     destination: "0x1234...".parse()?,
    ///     amount: dec!(100),
    ///     time: chrono::Utc::now().timestamp_millis() as u64,
    /// };
    ///
    /// let action = send.into_action(ARBITRUM_MAINNET_CHAIN_ID, Chain::Mainnet);
    /// ```
    #[must_use]
    pub fn into_action(self, chain: Chain) -> UsdSendAction {
        UsdSendAction {
            signature_chain_id: chain.arbitrum_id().to_owned(),
            hyperliquid_chain: chain,
            destination: self.destination,
            amount: self.amount,
            time: self.time,
        }
    }
}

/// Send spot tokens (inner data).
///
/// This is the core data structure for a spot token transfer. To create a signable action,
/// use the `into_action()` method to convert it to a `SpotSendAction`.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#core-spot-transfer>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotSend {
    /// The destination address.
    pub destination: Address,
    /// Token
    pub token: SendToken,
    /// The amount.
    pub amount: Decimal,
    /// Current time, should match the nonce
    pub time: u64,
}

impl SpotSend {
    /// Converts this into a signable `SpotSendAction`.
    ///
    /// # Parameters
    ///
    /// - `signature_chain_id`: The chain ID for signature verification (e.g., [`super::ARBITRUM_MAINNET_CHAIN_ID`])
    /// - `chain`: Whether this is mainnet or testnet
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let send = SpotSend {
    ///     destination: "0x1234...".parse()?,
    ///     token: SendToken(purr_token),
    ///     amount: dec!(1000),
    ///     time: chrono::Utc::now().timestamp_millis() as u64,
    /// };
    ///
    /// let action = send.into_action(ARBITRUM_MAINNET_CHAIN_ID, Chain::Mainnet);
    /// ```
    #[must_use]
    pub fn into_action(self, chain: Chain) -> SpotSendAction {
        SpotSendAction {
            signature_chain_id: chain.arbitrum_id().to_owned(),
            hyperliquid_chain: chain,
            destination: self.destination,
            token: self.token.to_string(),
            amount: self.amount,
            time: self.time,
        }
    }
}

/// Send asset between accounts or DEXes (inner data).
///
/// This is the core data structure for sending assets across different contexts
/// (e.g., between DEXes or subaccounts). To create a signable action,
/// use the `into_action()` method to convert it to a `SendAssetAction`.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#send-asset>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendAsset {
    /// The destination address.
    pub destination: Address,
    /// Source DEX, can be empty
    pub source_dex: String,
    /// Destiation DEX, can be empty
    pub destination_dex: String,
    /// Token
    pub token: SendToken,
    /// The amount.
    pub amount: Decimal,
    /// From subaccount, can be empty
    pub from_sub_account: String,
    /// Request nonce
    pub nonce: u64,
}

impl SendAsset {
    /// Converts this into a signable `SendAssetAction`.
    ///
    /// # Parameters
    ///
    /// - `signature_chain_id`: The chain ID for signature verification (e.g., [`super::ARBITRUM_MAINNET_CHAIN_ID`])
    /// - `chain`: Whether this is mainnet or testnet
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let send = SendAsset {
    ///     destination: "0x1234...".parse()?,
    ///     source_dex: String::new(),
    ///     destination_dex: String::new(),
    ///     token: SendToken(token),
    ///     amount: dec!(500),
    ///     from_sub_account: String::new(),
    ///     nonce: 12345,
    /// };
    ///
    /// let action = send.into_action(ARBITRUM_MAINNET_CHAIN_ID, Chain::Mainnet);
    /// ```
    #[must_use]
    pub fn into_action(self, chain: Chain) -> SendAssetAction {
        SendAssetAction {
            signature_chain_id: chain.arbitrum_id().to_owned(),
            hyperliquid_chain: chain,
            destination: self.destination,
            source_dex: self.source_dex,
            destination_dex: self.destination_dex,
            token: self.token.to_string(),
            amount: self.amount,
            from_sub_account: self.from_sub_account,
            nonce: self.nonce,
        }
    }
}

/// Response to an order insertion.
///
/// Contains the result of submitting an order to the exchange.
///
/// # Variants
///
/// - **Success**: Order was accepted (generic success)
/// - **Resting**: Order is resting on the book (not immediately filled)
/// - **Filled**: Order was immediately filled (market or aggressive limit)
/// - **Error**: Order was rejected with an error message
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::OrderResponseStatus;
///
/// # fn handle_order_response(status: OrderResponseStatus) {
/// match status {
///     OrderResponseStatus::Success => {
///         println!("Order accepted");
///     }
///     OrderResponseStatus::Resting { oid, cloid } => {
///         println!("Order {} resting on book", oid);
///     }
///     OrderResponseStatus::Filled { total_sz, avg_px, oid } => {
///         println!("Order {} filled: {} @ avg {}", oid, total_sz, avg_px);
///     }
///     OrderResponseStatus::Error(err) => {
///         eprintln!("Order rejected: {}", err);
///     }
/// }
/// # }
/// ```
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrderResponseStatus {
    /// Order accepted (generic)
    Success,
    /// Order resting on book
    Resting {
        /// Order ID
        oid: u64,
        /// Client order ID
        cloid: Option<B128>,
    },
    /// Order immediately filled
    Filled {
        /// Total filled size
        #[serde(rename = "totalSz")]
        total_sz: Decimal,
        /// Average fill price
        #[serde(rename = "avgPx")]
        avg_px: Decimal,
        /// Order ID
        oid: u64,
    },
    /// Order rejected with error
    Error(String),
}

impl OrderResponseStatus {
    /// Returns true if the order was successful (not an error).
    #[must_use]
    pub fn is_ok(&self) -> bool {
        !matches!(self, OrderResponseStatus::Error(_))
    }

    /// Returns true if the order resulted in an error.
    #[must_use]
    pub fn is_err(&self) -> bool {
        matches!(self, OrderResponseStatus::Error(_))
    }

    /// Returns the error message if this is an error response.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        match self {
            OrderResponseStatus::Error(err) => Some(err),
            _ => None,
        }
    }

    /// Returns the order ID if available (Resting or Filled).
    #[must_use]
    pub fn oid(&self) -> Option<u64> {
        match self {
            OrderResponseStatus::Resting { oid, .. } | OrderResponseStatus::Filled { oid, .. } => {
                Some(*oid)
            }
            _ => None,
        }
    }
}

/// Batch order submission.
///
/// A collection of orders sent together in a single transaction, optionally grouped
/// for atomic execution (e.g., bracket orders with take-profit and stop-loss).
///
/// # When to Use
///
/// - **Single order**: Use a vec with one [`OrderRequest`]
/// - **Multiple independent orders**: Set `grouping` to [`OrderGrouping::Na`]
/// - **Bracket orders (TP/SL)**: Use [`OrderGrouping::NormalTpsl`] or [`OrderGrouping::PositionTpsl`]
///
/// # Related Types
///
/// - [`OrderRequest`]: Individual order within the batch
/// - [`OrderGrouping`]: Grouping strategy for the batch
/// - [`OrderResponseStatus`]: Response status for each order
/// - [`HttpClient::place`](crate::hypercore::http::Client::place): Method to submit orders
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore::types::*;
/// use rust_decimal::dec;
///
/// let order = BatchOrder {
///     orders: vec![
///         OrderRequest {
///             asset: 0, // BTC
///             is_buy: true,
///             limit_px: dec!(50000),
///             sz: dec!(0.1),
///             reduce_only: false,
///             order_type: OrderTypePlacement::Limit {
///                 tif: TimeInForce::Gtc,
///             },
///             cloid: Default::default(),
///         }
///     ],
///     grouping: OrderGrouping::Na,
/// };
/// ```
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchOrder {
    pub orders: Vec<OrderRequest>,
    pub grouping: OrderGrouping,
}

/// Order grouping strategy.
///
/// Determines how orders are grouped when sent in a batch.
///
/// # Variants
///
/// - `Na` – No special grouping; orders are independent.
/// - `NormalTpsl` – Link a main order with its take-profit/stop-loss orders.
/// - `PositionTpsl` – Attach TP/SL orders to an existing position.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum OrderGrouping {
    Na,
    NormalTpsl,
    PositionTpsl,
}

/// A single order to be placed on the exchange.
///
/// Used as an element of [`BatchOrder::orders`] when submitting one or more
/// orders in a single request.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[serde_as]
pub struct OrderRequest {
    /// Asset index identifying the trading pair.
    #[serde(rename = "a")]
    pub asset: usize,
    /// `true` for a buy (bid), `false` for a sell (ask).
    #[serde(rename = "b")]
    pub is_buy: bool,
    /// Limit price for the order.
    #[serde(rename = "p", with = "rust_decimal::serde::str")]
    pub limit_px: Decimal,
    /// Order size in base asset units.
    #[serde(rename = "s", with = "rust_decimal::serde::str")]
    pub sz: Decimal,
    /// When `true`, the order can only reduce an existing position.
    #[serde(rename = "r")]
    pub reduce_only: bool,
    /// Order type specifying limit or trigger parameters.
    #[serde(rename = "t")]
    pub order_type: OrderTypePlacement,
    /// Client-supplied order ID for tracking.
    #[serde(rename = "c")]
    #[serde(
        serialize_with = "super::utils::serialize_cloid_as_hex",
        deserialize_with = "super::utils::deserialize_cloid_from_hex"
    )]
    pub cloid: Cloid,
}

/// Order type for the placement.
///
/// Specifies whether the order is limit or trigger and its associated parameters.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum OrderTypePlacement {
    Limit {
        tif: TimeInForce,
    },
    #[serde(rename_all = "camelCase")]
    Trigger {
        is_market: bool,
        #[serde(with = "rust_decimal::serde::str")]
        trigger_px: Decimal,
        tpsl: TpSl,
    },
}

/// Trigger type.
///
/// Indicates whether the trigger is a take‑profit (`Tp`) or stop‑loss (`Sl`).
#[derive(PartialEq, Eq, Deserialize, Serialize, Copy, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum TpSl {
    Tp,
    Sl,
}

/// Batch modify request.
///
/// Contains a list of order modifications to be applied atomically.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchModify {
    /// The modifications to apply.
    pub modifies: Vec<Modify>,
}

/// Modification of an existing order.
///
/// Identifies the order to modify (by exchange-assigned ID or client ID) and
/// provides the replacement order parameters.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Modify {
    /// Order identifier – either a numeric `oid` or a client-supplied `cloid`.
    #[serde(with = "either::serde_untagged")]
    pub oid: OidOrCloid,
    /// New order parameters that will replace the existing order.
    pub order: OrderRequest,
}

/// Batch cancel request.
///
/// Contains a list of order IDs to cancel.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BatchCancel {
    pub cancels: Vec<Cancel>,
}

/// Batch cancel by cloid request.
///
/// Contains a list of cloid values to cancel.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BatchCancelCloid {
    pub cancels: Vec<CancelByCloid>,
}

/// Cancel request for a single order.
///
/// Identifies the order to cancel by its asset index and exchange-assigned ID.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Cancel {
    /// Asset index the order belongs to.
    #[serde(rename = "a")]
    pub asset: usize,
    /// Exchange-assigned order ID.
    #[serde(rename = "o")]
    pub oid: u64,
}

/// Cancel request by cloid.
///
/// References an order by asset and cloid.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CancelByCloid {
    pub asset: u32,
    #[serde(with = "const_hex")]
    pub cloid: B128,
}

/// Schedule cancellation of all orders.
///
/// The optional `time` field can be used to delay the cancellation.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleCancel {
    pub time: Option<u64>,
}

/// Clearinghouse state for a user's perpetual positions.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
/// use hypersdk::Address;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = hypercore::mainnet();
/// let user: Address = "0x...".parse()?;
/// let state = client.clearinghouse_state(user).await?;
///
/// println!("Account value: {}", state.margin_summary.account_value);
/// println!("Withdrawable: {}", state.withdrawable);
///
/// for position in &state.asset_positions {
///     println!("{}: {} @ {:?}",
///         position.position.coin,
///         position.position.szi,
///         position.position.entry_px
///     );
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearinghouseState {
    /// Margin summary for isolated positions
    pub margin_summary: MarginSummary,
    /// Margin summary for cross-margin account
    pub cross_margin_summary: MarginSummary,
    /// Cross maintenance margin used
    pub cross_maintenance_margin_used: Decimal,
    /// Amount available for withdrawal
    pub withdrawable: Decimal,
    /// List of asset positions
    pub asset_positions: Vec<AssetPosition>,
    /// Timestamp in milliseconds
    pub time: u64,
}

/// Margin summary for an account.
///
/// Contains aggregate margin information for either isolated or cross-margin positions.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarginSummary {
    /// Total account value (equity)
    pub account_value: Decimal,
    /// Total notional position value
    pub total_ntl_pos: Decimal,
    /// Total raw USD value
    pub total_raw_usd: Decimal,
    /// Total margin used
    pub total_margin_used: Decimal,
}

impl MarginSummary {
    /// Returns the available margin (account value - margin used).
    #[must_use]
    pub fn available_margin(&self) -> Decimal {
        self.account_value - self.total_margin_used
    }

    /// Returns the margin utilization as a percentage.
    ///
    /// Returns 0 if account value is zero.
    #[must_use]
    pub fn margin_utilization(&self) -> Decimal {
        if self.account_value.is_zero() {
            Decimal::ZERO
        } else {
            (self.total_margin_used / self.account_value) * Decimal::ONE_HUNDRED
        }
    }
}

/// Position type for perpetual positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, derive_more::Display)]
#[serde(rename_all = "camelCase")]
pub enum PositionType {
    /// One-way position mode (single position per market)
    #[display("oneWay")]
    OneWay,
}

/// A user's position in a specific asset.
///
/// Wraps the position details along with cumulative funding information.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPosition {
    /// Type of position
    #[serde(rename = "type")]
    pub position_type: PositionType,
    /// Detailed position information
    pub position: PositionData,
}

/// Detailed position data for an asset.
///
/// Contains all information about a single perpetual position.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionData {
    /// Asset/coin symbol (e.g., "BTC", "ETH")
    pub coin: String,
    /// Position size (positive for long, negative for short)
    pub szi: Decimal,
    /// Leverage configuration
    pub leverage: Leverage,
    /// Entry price
    pub entry_px: Option<Decimal>,
    /// Current position value
    pub position_value: Decimal,
    /// Unrealized profit and loss
    pub unrealized_pnl: Decimal,
    /// Return on equity (as a decimal, e.g., 0.05 for 5%)
    pub return_on_equity: Decimal,
    /// Liquidation price (None if no position)
    pub liquidation_px: Option<Decimal>,
    /// Margin used for this position
    pub margin_used: Decimal,
    /// Maximum leverage allowed for this asset
    pub max_leverage: u32,
    /// Cumulative funding payments
    pub cum_funding: CumulativeFunding,
}

impl PositionData {
    /// Returns true if this is a long position.
    #[must_use]
    pub fn is_long(&self) -> bool {
        self.szi > Decimal::ZERO
    }

    /// Returns true if this is a short position.
    #[must_use]
    pub fn is_short(&self) -> bool {
        self.szi < Decimal::ZERO
    }

    /// Returns the absolute position size.
    #[must_use]
    pub fn abs_size(&self) -> Decimal {
        self.szi.abs()
    }

    /// Returns the position side as a string ("long" or "short").
    #[must_use]
    pub fn side(&self) -> &'static str {
        if self.is_long() { "long" } else { "short" }
    }
}

/// Leverage type for positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, derive_more::Display)]
#[serde(rename_all = "camelCase")]
pub enum LeverageType {
    /// Cross-margin mode (shared margin across positions)
    #[display("cross")]
    Cross,
    /// Isolated-margin mode (dedicated margin per position)
    #[display("isolated")]
    Isolated,
}

/// Leverage configuration for a position.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leverage {
    /// Leverage type
    #[serde(rename = "type")]
    pub leverage_type: LeverageType,
    /// Leverage value (e.g., 10 for 10x)
    pub value: u32,
    /// Raw USD value used for isolated leverage (if applicable)
    #[serde(default)]
    #[serde(with = "rust_decimal::serde::str_option")]
    pub raw_usd: Option<Decimal>,
}

impl Leverage {
    /// Returns true if this is cross-margin leverage.
    #[must_use]
    pub fn is_cross(&self) -> bool {
        self.leverage_type == LeverageType::Cross
    }

    /// Returns true if this is isolated-margin leverage.
    #[must_use]
    pub fn is_isolated(&self) -> bool {
        self.leverage_type == LeverageType::Isolated
    }
}

/// Cumulative funding payments for a position.
///
/// Tracks funding payments over different time periods.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CumulativeFunding {
    /// Total funding payments since position opened
    pub all_time: Decimal,
    /// Funding payments since position was opened
    pub since_open: Decimal,
    /// Funding payments since last position change
    pub since_change: Decimal,
}

/// Historical funding rate record.
///
/// Represents a single funding rate snapshot for a perpetual market.
/// Funding rates are typically applied every 8 hours.
///
/// # Fields
///
/// - `coin`: Market symbol (e.g., "BTC", "ETH")
/// - `funding_rate`: The funding rate applied to positions (decimal format)
/// - `premium`: Market premium component used in funding calculation
/// - `time`: Unix timestamp in milliseconds when the rate was applied
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = hypercore::mainnet();
/// let start_time = 1681923833000u64;
/// let rates = client.funding_history("BTC", start_time, None).await?;
///
/// for rate in rates {
///     println!("{} funding rate at {}: {} (premium: {})",
///         rate.coin, rate.time, rate.funding_rate, rate.premium);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingRate {
    /// Market symbol (e.g., "BTC", "ETH")
    pub coin: String,
    /// Funding rate applied to positions
    #[serde(with = "rust_decimal::serde::str")]
    pub funding_rate: Decimal,
    /// Market premium component
    #[serde(with = "rust_decimal::serde::str")]
    pub premium: Decimal,
    /// Timestamp in milliseconds
    pub time: u64,
}

impl FundingRate {
    /// Returns the annualized funding rate (assuming 3 funding periods per day).
    ///
    /// Funding typically occurs every 8 hours (3 times per day).
    #[must_use]
    pub fn annualized_rate(&self) -> Decimal {
        self.funding_rate * Decimal::from(3 * 365)
    }

    /// Returns true if the funding rate is positive (longs pay shorts).
    #[must_use]
    pub fn is_positive(&self) -> bool {
        self.funding_rate > Decimal::ZERO
    }

    /// Returns true if the funding rate is negative (shorts pay longs).
    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.funding_rate < Decimal::ZERO
    }
}

/// User balance.
///
/// Represents the balance of a specific token in a user's account.
///
/// # Fields
///
/// - `coin`: Token symbol (e.g., "USDC", "BTC")
/// - `token`: Token index in the system
/// - `hold`: Amount currently held (locked in orders or positions)
/// - `total`: Total balance (held + available)
/// - `entry_ntl`: Entry notional value for position tracking
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::UserBalance;
/// use rust_decimal::dec;
///
/// # fn check_balance(balance: UserBalance) {
/// // Check available balance
/// let available = balance.available();
/// println!("Available {}: {}", balance.coin, available);
///
/// // Check if sufficient balance for trade
/// let trade_amount = dec!(100);
/// if balance.can_trade(trade_amount) {
///     println!("Sufficient balance for trade");
/// } else {
///     println!("Insufficient balance");
/// }
/// # }
/// ```
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserBalance {
    /// Token symbol
    pub coin: String,
    /// Token index
    pub token: usize,
    /// Amount held (locked)
    pub hold: Decimal,
    /// Total balance
    pub total: Decimal,
    /// Entry notional
    pub entry_ntl: Decimal,
}

impl UserBalance {
    /// Returns the available balance (total - hold).
    ///
    /// This is the amount that can be freely used for new orders or withdrawals.
    #[must_use]
    pub fn available(&self) -> Decimal {
        self.total - self.hold
    }

    /// Returns true if the available balance is sufficient for the given amount.
    #[must_use]
    pub fn can_trade(&self, amount: Decimal) -> bool {
        self.available() >= amount
    }

    /// Returns true if there is any held balance.
    #[must_use]
    pub fn has_held(&self) -> bool {
        self.hold > Decimal::ZERO
    }

    /// Returns the percentage of balance that is held (locked).
    ///
    /// Returns a Decimal (e.g., 25.5 for 25.5%). Returns 0 if total balance is zero.
    #[must_use]
    pub fn held_percentage(&self) -> Decimal {
        if self.total.is_zero() {
            Decimal::ZERO
        } else {
            (self.hold / self.total) * Decimal::ONE_HUNDRED
        }
    }
}

/// Abstraction over a token to be sent out.
///
/// This is to prevent users from f*cking it up.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendToken(pub SpotToken);

impl fmt::Display for SendToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // write!(f, "{}:{:x}", self.0.name, self.0.token_id)
        write!(f, "{}", self.0.name)
    }
}

/// Multi-signature wallet configuration.
///
/// Defines the authorized signers and threshold for a multisig account on Hyperliquid.
/// A multisig account requires a minimum number of signatures (threshold) from the
/// authorized users to execute transactions.
///
/// # Fields
///
/// - `authorized_users`: List of addresses authorized to sign transactions for this multisig
/// - `threshold`: Minimum number of signatures required to execute a transaction
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::MultiSigConfig;
///
/// # fn example(config: MultiSigConfig) {
/// // Check if enough signers are authorized
/// assert!(config.threshold <= config.authorized_users.len());
///
/// println!("Multisig requires {} of {} signatures",
///     config.threshold,
///     config.authorized_users.len()
/// );
/// # }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiSigConfig {
    /// Addresses authorized to sign for this multisig account
    pub authorized_users: Vec<Address>,
    /// Minimum number of signatures required (e.g., 2 for 2-of-3)
    pub threshold: usize,
}

/// Extra agent information.
///
/// Represents an additional agent authorized to act on behalf of a user account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiAgent {
    /// Name or identifier of the agent
    pub name: String,
    /// Address of the agent
    pub address: Address,
    /// Timestamp in milliseconds until which this agent is valid
    pub valid_until: Option<u64>,
}

/// Role of a user in the Hyperliquid system.
///
/// Returned by the `userRole` info endpoint to identify what type of account
/// a given address represents.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
/// use hypersdk::Address;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = hypercore::mainnet();
/// let addr: Address = "0x...".parse()?;
/// let role = client.user_role(addr).await?;
///
/// match role {
///     hypersdk::hypercore::types::UserRole::User => println!("Regular user"),
///     hypersdk::hypercore::types::UserRole::Vault => println!("Vault account"),
///     hypersdk::hypercore::types::UserRole::Agent { user } => {
///         println!("Agent wallet for {}", user);
///     }
///     _ => {}
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", content = "data", rename_all = "camelCase")]
pub enum UserRole {
    /// Regular user account
    User,
    /// Agent wallet authorized to act on behalf of another account
    Agent {
        /// The main user address this agent acts on behalf of
        user: Address,
    },
    /// Vault account
    Vault,
    /// Subaccount
    SubAccount { master: Address },
    /// Address not found in the system
    Missing,
}

/// User's equity in a vault.
///
/// Represents a user's deposit and equity position in a specific vault.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
/// use hypersdk::Address;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = hypercore::mainnet();
/// let user: Address = "0x...".parse()?;
/// let equities = client.user_vault_equities(user).await?;
///
/// for equity in equities {
///     println!("Vault {:?}: equity = {}", equity.vault_address, equity.equity);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserVaultEquity {
    /// The vault address
    pub vault_address: Address,
    /// User's equity in the vault
    pub equity: Decimal,
    /// Timestamp until which funds are locked
    pub locked_until_timestamp: Option<u64>,
}

/// Vault details response.
///
/// Contains comprehensive information about a vault including performance metrics,
/// follower information, and configuration.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint#retrieve-details-for-a-vault>
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultDetails {
    /// Name of the vault
    pub name: String,
    /// Address of the vault
    pub vault_address: Address,
    /// Leader (manager) of the vault
    pub leader: Address,
    /// Description of the vault
    pub description: String,
    /// Portfolio performance data for different time periods
    pub portfolio: Vec<(String, VaultPortfolio)>,
    /// Annual percentage return
    pub apr: Decimal,
    /// State of the current user as a follower (if queried with user parameter)
    pub follower_state: Option<VaultFollowerState>,
    /// Leader's fraction of the vault
    pub leader_fraction: Decimal,
    /// Leader's commission rate
    pub leader_commission: Decimal,
    /// List of vault followers
    pub followers: Vec<VaultFollower>,
    /// Maximum amount that can be distributed
    pub max_distributable: Decimal,
    /// Maximum amount that can be withdrawn
    pub max_withdrawable: Decimal,
    /// Whether the vault is closed
    #[serde(default)]
    pub is_closed: bool,
    /// Relationship type
    #[serde(default)]
    pub relationship: Option<VaultRelationship>,
    /// Whether the vault allows deposits
    #[serde(default)]
    pub allow_deposits: bool,
    /// Whether to always close on withdraw
    #[serde(default)]
    pub always_close_on_withdraw: bool,
}

/// Vault relationship type.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRelationship {
    /// Type of relationship
    #[serde(rename = "type")]
    pub relationship_type: VaultRelationshipType,
}

/// Type of vault relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display)]
#[serde(rename_all = "lowercase")]
pub enum VaultRelationshipType {
    /// Normal vault relationship
    Normal,
}

/// Vault portfolio data for a specific time period.
///
/// Contains historical account value and PnL data.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultPortfolio {
    /// Historical account values as (timestamp_ms, value) pairs
    pub account_value_history: Vec<(u64, String)>,
    /// Historical PnL values as (timestamp_ms, value) pairs
    pub pnl_history: Vec<(u64, String)>,
    /// Volume for the period
    pub vlm: String,
}

/// State of a user as a vault follower.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultFollowerState {
    /// User's equity in the vault
    pub vault_equity: Decimal,
    /// User's PnL
    pub pnl: Decimal,
    /// User's all-time PnL
    pub all_time_pnl: Decimal,
    /// Number of days following
    pub days_following: u64,
    /// Timestamp when user entered the vault
    pub vault_entry_time: u64,
    /// Timestamp until which funds are locked (if any)
    pub lockup_until: Option<u64>,
}

/// Information about a vault follower.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultFollower {
    /// Follower's identity (address or special role like Leader)
    pub user: VaultFollowerUser,
    /// Follower's equity in the vault
    pub vault_equity: Decimal,
    /// Follower's PnL
    pub pnl: Decimal,
    /// Follower's all-time PnL
    pub all_time_pnl: Decimal,
    /// Number of days following
    pub days_following: u64,
    /// Timestamp when user entered the vault
    pub vault_entry_time: u64,
    /// Timestamp until which funds are locked (if any)
    pub lockup_until: Option<u64>,
}

/// Identity of a vault follower.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultFollowerUser {
    /// The vault leader
    Leader,
    /// A regular follower address
    Address(Address),
}

impl fmt::Display for VaultFollowerUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultFollowerUser::Leader => write!(f, "Leader"),
            VaultFollowerUser::Address(addr) => write!(f, "{:?}", addr),
        }
    }
}

impl<'de> serde::Deserialize<'de> for VaultFollowerUser {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        if s == "Leader" {
            Ok(VaultFollowerUser::Leader)
        } else {
            s.parse::<Address>()
                .map(VaultFollowerUser::Address)
                .map_err(serde::de::Error::custom)
        }
    }
}

// ========================================================
// SUBACCOUNT TYPES
// ========================================================

/// A user's subaccount with state information.
///
/// Represents a subaccount associated with a master account, including its
/// clearinghouse state (perpetuals) and spot balances.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
/// use hypersdk::Address;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = hypercore::mainnet();
/// let master: Address = "0x...".parse()?;
///
/// let subaccounts = client.subaccounts(master).await?;
/// for sub in subaccounts {
///     println!("Subaccount '{}': {:?}", sub.name, sub.sub_account_user);
///     println!("  Account value: {}", sub.clearinghouse_state.margin_summary.account_value);
/// }
/// # Ok(())
/// # }
/// ```
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint#retrieve-a-users-subaccounts>
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAccount {
    /// Human-readable name of the subaccount
    pub name: String,
    /// Address of the subaccount
    pub sub_account_user: Address,
    /// Address of the master account
    pub master: Address,
    /// Clearinghouse state for perpetuals trading
    pub clearinghouse_state: ClearinghouseState,
    /// Spot trading state
    pub spot_state: SpotState,
}

/// Spot trading state for an account.
///
/// Contains the spot balances for an account.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotState {
    /// List of spot balances
    pub balances: Vec<UserBalance>,
}

/// Signature.
///
/// Represents an EIP‑712 signature split into its components.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde_as]
pub struct Signature {
    #[serde(
        serialize_with = "super::utils::serialize_as_hex",
        deserialize_with = "super::utils::deserialize_from_hex"
    )]
    pub r: U256,
    #[serde(
        serialize_with = "super::utils::serialize_as_hex",
        deserialize_with = "super::utils::deserialize_from_hex"
    )]
    pub s: U256,
    pub v: u64,
}

impl fmt::Display for Signature {
    /// Formats the signature as a hex string in the format: 0x{r}{s}{v}
    ///
    /// This is the standard Ethereum signature format where:
    /// - r: 32 bytes (64 hex chars)
    /// - s: 32 bytes (64 hex chars)
    /// - v: 1 byte (2 hex chars)
    ///
    /// Total: 130 hex characters (0x prefix + 128 chars)
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:064x}{:064x}{:02x}", self.r, self.s, self.v)
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Signature")
            .field("r", &format!("0x{:x}", self.r))
            .field("s", &format!("0x{:x}", self.s))
            .field("v", &self.v)
            .finish()
    }
}

impl std::str::FromStr for Signature {
    type Err = anyhow::Error;

    /// Parses a signature from a hex string.
    ///
    /// The input can be:
    /// - With or without "0x" prefix
    /// - 130 hex characters (65 bytes: r=32, s=32, v=1)
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Remove 0x prefix if present
        let hex_str = s.strip_prefix("0x").unwrap_or(s);

        // Validate length (130 hex chars = 65 bytes)
        if hex_str.len() != 130 {
            anyhow::bail!(
                "Invalid signature length: expected 130 hex characters (65 bytes), got {}",
                hex_str.len()
            );
        }

        // Parse r (first 64 hex chars = 32 bytes)
        let r = U256::from_str_radix(&hex_str[..64], 16)
            .map_err(|e| anyhow::anyhow!("Failed to parse r component: {}", e))?;

        // Parse s (next 64 hex chars = 32 bytes)
        let s = U256::from_str_radix(&hex_str[64..128], 16)
            .map_err(|e| anyhow::anyhow!("Failed to parse s component: {}", e))?;

        // Parse v (last 2 hex chars = 1 byte)
        let v = u64::from_str_radix(&hex_str[128..130], 16)
            .map_err(|e| anyhow::anyhow!("Failed to parse v component: {}", e))?;

        Ok(Signature { r, s, v })
    }
}

impl From<Signature> for alloy::signers::Signature {
    fn from(sig: Signature) -> Self {
        let recid = RecoveryId::from_byte((sig.v - 27) as u8).expect("recid");
        Self::new(sig.r, sig.s, recid.is_y_odd())
    }
}

impl From<alloy::signers::Signature> for Signature {
    fn from(signature: alloy::signers::Signature) -> Self {
        let recid = signature.recid().to_byte() as u64 + 27;
        Self {
            r: signature.r(),
            s: signature.s(),
            v: recid,
        }
    }
}

/// Candle snapshot request parameters.
///
/// Used to query historical candlestick data from the API.
///
/// # Notes
///
/// - Only the most recent 5000 candles are available
/// - Times are in milliseconds
/// - For HIP-3 assets, prefix the coin with dex name (e.g., "xyz:XYZ100")
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CandleSnapshotRequest {
    /// Market symbol (e.g., "BTC", "ETH")
    pub coin: String,
    /// Candle interval (e.g., "1m", "15m", "1h", "1d")
    pub interval: CandleInterval,
    /// Start time in milliseconds
    pub start_time: u64,
    /// End time in milliseconds
    pub end_time: u64,
}

// ========================================================
// PRIVATE TYPES
// ========================================================

/// Info endpoint request types.
///
/// Used for querying various types of information from the API.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub(super) enum InfoRequest {
    Meta {
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    SpotMeta,
    PerpDexs,
    FrontendOpenOrders {
        user: Address,
    },
    HistoricalOrders {
        user: Address,
    },
    UserFills {
        user: Address,
    },
    OrderStatus {
        user: Address,
        #[serde(with = "either::serde_untagged")]
        oid: OidOrCloid,
    },
    SpotClearinghouseState {
        user: Address,
    },
    ClearinghouseState {
        user: Address,
    },
    AllMids {
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    CandleSnapshot {
        req: CandleSnapshotRequest,
    },
    UserToMultiSigSigners {
        user: Address,
    },
    ExtraAgents {
        user: Address,
    },
    FundingHistory {
        coin: String,
        #[serde(rename = "startTime")]
        start_time: u64,
        #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
        end_time: Option<u64>,
    },
    /// Retrieve details for a vault.
    VaultDetails {
        #[serde(rename = "vaultAddress")]
        vault_address: Address,
        #[serde(skip_serializing_if = "Option::is_none")]
        user: Option<Address>,
    },
    /// Retrieve a user's vault deposits.
    UserVaultEquities {
        user: Address,
    },
    /// Query a user's role.
    UserRole {
        user: Address,
    },
    /// Retrieve a user's subaccounts.
    SubAccounts {
        user: Address,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hypercore::types::api::Response;

    #[test]
    fn test_api_error_response() {
        let text = r#"{
           "status":"ok",
           "response":{
              "type":"order",
              "data":{
                 "statuses":[
                    {
                       "error":"Order must have minimum value of $10."
                    }
                 ]
              }
           }
        }"#;
        let res = serde_json::from_str::<Response>(text);
        assert!(res.is_ok());
    }

    #[test]
    fn test_api_order_response() {
        let text = r#"{
           "status":"ok",
           "response":{
              "type":"order",
              "data":{
                 "statuses":[
                    {
                       "resting":{
                          "oid":77738308
                       }
                    }
                 ]
              }
           }
        }"#;
        let res = serde_json::from_str::<Response>(text);
        assert!(res.is_ok());
    }

    #[test]
    fn test_signature_from_str_with_0x_prefix() {
        let hex_sig = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1b";
        let sig: Signature = hex_sig.parse().unwrap();

        assert_eq!(
            sig.r,
            U256::from_str_radix(
                "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                16
            )
            .unwrap()
        );
        assert_eq!(
            sig.s,
            U256::from_str_radix(
                "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                16
            )
            .unwrap()
        );
        assert_eq!(sig.v, 27);
    }

    #[test]
    fn test_signature_from_str_without_0x_prefix() {
        let hex_sig = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1b";
        let sig: Signature = hex_sig.parse().unwrap();

        assert_eq!(
            sig.r,
            U256::from_str_radix(
                "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                16
            )
            .unwrap()
        );
        assert_eq!(
            sig.s,
            U256::from_str_radix(
                "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                16
            )
            .unwrap()
        );
        assert_eq!(sig.v, 27);
    }

    #[test]
    fn test_candle_interval_display() {
        assert_eq!(CandleInterval::OneMinute.to_string(), "1m");
        assert_eq!(CandleInterval::FifteenMinutes.to_string(), "15m");
        assert_eq!(CandleInterval::OneHour.to_string(), "1h");
        assert_eq!(CandleInterval::OneDay.to_string(), "1d");
        assert_eq!(CandleInterval::OneWeek.to_string(), "1w");
        assert_eq!(CandleInterval::OneMonth.to_string(), "1M");
    }

    #[test]
    fn test_candle_interval_from_str() {
        assert_eq!(
            "1m".parse::<CandleInterval>().unwrap(),
            CandleInterval::OneMinute
        );
        assert_eq!(
            "15m".parse::<CandleInterval>().unwrap(),
            CandleInterval::FifteenMinutes
        );
        assert_eq!(
            "1h".parse::<CandleInterval>().unwrap(),
            CandleInterval::OneHour
        );
        assert_eq!(
            "4h".parse::<CandleInterval>().unwrap(),
            CandleInterval::FourHours
        );
        assert_eq!(
            "1d".parse::<CandleInterval>().unwrap(),
            CandleInterval::OneDay
        );
        assert_eq!(
            "1w".parse::<CandleInterval>().unwrap(),
            CandleInterval::OneWeek
        );
        assert_eq!(
            "1M".parse::<CandleInterval>().unwrap(),
            CandleInterval::OneMonth
        );
    }

    #[test]
    fn test_candle_interval_from_str_invalid() {
        let result = "invalid".parse::<CandleInterval>();
        assert!(result.is_err());
    }

    #[test]
    fn test_candle_deserialization() {
        let json = r#"{
            "t": 1681923600000,
            "T": 1681924499999,
            "s": "BTC",
            "i": "15m",
            "o": "29295.0",
            "h": "29309.0",
            "l": "29250.0",
            "c": "29258.0",
            "v": "0.98639",
            "n": 189
        }"#;

        let candle: Candle = serde_json::from_str(json).unwrap();
        assert_eq!(candle.open_time, 1681923600000);
        assert_eq!(candle.close_time, 1681924499999);
        assert_eq!(candle.coin, "BTC");
        assert_eq!(candle.interval, "15m");
        assert_eq!(candle.open.to_string(), "29295.0");
        assert_eq!(candle.high.to_string(), "29309.0");
        assert_eq!(candle.low.to_string(), "29250.0");
        assert_eq!(candle.close.to_string(), "29258.0");
        assert_eq!(candle.volume.to_string(), "0.98639");
        assert_eq!(candle.num_trades, 189);
    }

    #[test]
    fn test_candle_subscription() {
        let sub = Subscription::Candle {
            coin: "BTC".to_string(),
            interval: "1m".to_string(),
        };

        let json = serde_json::to_string(&sub).unwrap();
        let deserialized: Subscription = serde_json::from_str(&json).unwrap();
        assert_eq!(sub, deserialized);
    }

    #[test]
    fn test_incoming_candle() {
        let json = r#"{
            "channel": "candle",
            "data": {
                "t": 1681923600000,
                "T": 1681924499999,
                "s": "ETH",
                "i": "1h",
                "o": "1850.5",
                "h": "1855.0",
                "l": "1848.0",
                "c": "1852.3",
                "v": "125.45",
                "n": 450
            }
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::Candle(candle) => {
                assert_eq!(candle.coin, "ETH");
                assert_eq!(candle.interval, "1h");
                assert_eq!(candle.open.to_string(), "1850.5");
                assert_eq!(candle.close.to_string(), "1852.3");
            }
            _ => panic!("Expected Incoming::Candle"),
        }
    }

    #[test]
    fn test_signature_from_str_invalid_length() {
        let hex_sig = "0x1234"; // Too short
        let result: Result<Signature, _> = hex_sig.parse();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid signature length")
        );
    }

    #[test]
    fn test_signature_from_str_invalid_hex() {
        let hex_sig = "0xGGGG567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1b";
        let result: Result<Signature, _> = hex_sig.parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_signature_display_format() {
        let sig = Signature {
            r: U256::from_str_radix(
                "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                16,
            )
            .unwrap(),
            s: U256::from_str_radix(
                "fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321",
                16,
            )
            .unwrap(),
            v: 28,
        };

        let display_str = sig.to_string();
        assert!(display_str.starts_with("0x"));
        assert_eq!(display_str.len(), 132); // 0x + 64 + 64 + 2 = 132
        assert!(
            display_str
                .contains("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
        );
        assert!(
            display_str
                .contains("fedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321")
        );
        assert!(display_str.ends_with("1c")); // v=28 = 0x1c
    }

    #[test]
    fn test_signature_roundtrip() {
        let original = Signature {
            r: U256::from_str_radix(
                "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                16,
            )
            .unwrap(),
            s: U256::from_str_radix(
                "0987654321fedcba0987654321fedcba0987654321fedcba0987654321fedcba",
                16,
            )
            .unwrap(),
            v: 27,
        };

        // Convert to string and back
        let sig_str = original.to_string();
        let parsed: Signature = sig_str.parse().unwrap();

        assert_eq!(original.r, parsed.r);
        assert_eq!(original.s, parsed.s);
        assert_eq!(original.v, parsed.v);
    }

    #[test]
    fn test_clearinghouse_state_deserialization() {
        let json = r#"{"marginSummary":{"accountValue":"8272576.5729350001","totalNtlPos":"9077249.2563109994","totalRawUsd":"8099875.5474460004","totalMarginUsed":"1120386.813659"},"crossMarginSummary":{"accountValue":"8259027.0754620004","totalNtlPos":"9038408.6103639994","totalRawUsd":"8047485.4040259998","totalMarginUsed":"1106837.3161859999"},"crossMaintenanceMarginUsed":"356978.709123","withdrawable":"6286581.8806220004","assetPositions":[{"type":"oneWay","position":{"coin":"BTC","szi":"-1.47472","leverage":{"type":"cross","value":20},"entryPx":"95137.8","positionValue":"140406.61648","unrealizedPnl":"-104.935956","returnOnEquity":"-0.0149586171","liquidationPx":"5387394.7801264981","marginUsed":"7020.330824","maxLeverage":40,"cumFunding":{"allTime":"-179748.281779","sinceOpen":"0.0","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"ETH","szi":"-45.7436","leverage":{"type":"cross","value":20},"entryPx":"3297.47","positionValue":"151232.91596","unrealizedPnl":"-394.470067","returnOnEquity":"-0.0523036504","liquidationPx":"172665.4473515121","marginUsed":"7561.645798","maxLeverage":25,"cumFunding":{"allTime":"-131967.431285","sinceOpen":"-1.52718","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"SOL","szi":"30390.93","leverage":{"type":"cross","value":20},"entryPx":"144.1206","positionValue":"4398175.3896000003","unrealizedPnl":"18214.531954","returnOnEquity":"0.0831721221","liquidationPx":null,"marginUsed":"219908.76948","maxLeverage":20,"cumFunding":{"allTime":"-142932.239953","sinceOpen":"817.466593","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"LTC","szi":"3.51","leverage":{"type":"cross","value":10},"entryPx":"98.87","positionValue":"277.72875","unrealizedPnl":"-69.30495","returnOnEquity":"-1.9970668555","liquidationPx":null,"marginUsed":"27.772875","maxLeverage":10,"cumFunding":{"allTime":"-866.777178","sinceOpen":"4.951526","sinceChange":"4.951526"}}},{"type":"oneWay","position":{"coin":"LDO","szi":"16332.0","leverage":{"type":"cross","value":10},"entryPx":"0.66227","positionValue":"10661.85624","unrealizedPnl":"-154.358374","returnOnEquity":"-0.142710162","liquidationPx":null,"marginUsed":"1066.185624","maxLeverage":10,"cumFunding":{"allTime":"-911.231239","sinceOpen":"0.432907","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"XRP","szi":"-92720.0","leverage":{"type":"cross","value":20},"entryPx":"2.127177","positionValue":"197317.432","unrealizedPnl":"-85.535846","returnOnEquity":"-0.0086736322","liquidationPx":"85.2742980086","marginUsed":"9865.8716","maxLeverage":20,"cumFunding":{"allTime":"-37019.125174","sinceOpen":"-7.576659","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"WIF","szi":"146.0","leverage":{"type":"cross","value":5},"entryPx":"0.344551","positionValue":"60.85864","unrealizedPnl":"10.55408","returnOnEquity":"1.0490182202","liquidationPx":null,"marginUsed":"12.171728","maxLeverage":5,"cumFunding":{"allTime":"-406.325071","sinceOpen":"0.168658","sinceChange":"0.168658"}}},{"type":"oneWay","position":{"coin":"SAGA","szi":"-220.2","leverage":{"type":"cross","value":3},"entryPx":"0.10448","positionValue":"13.899024","unrealizedPnl":"9.107472","returnOnEquity":"1.1875957121","liquidationPx":"30759.3016032192","marginUsed":"4.633008","maxLeverage":3,"cumFunding":{"allTime":"-1.45675","sinceOpen":"0.17651","sinceChange":"0.17651"}}},{"type":"oneWay","position":{"coin":"MOODENG","szi":"54674.0","leverage":{"type":"cross","value":3},"entryPx":"0.084892","positionValue":"4618.58615","unrealizedPnl":"-22.823047","returnOnEquity":"-0.0147518002","liquidationPx":null,"marginUsed":"1539.528716","maxLeverage":3,"cumFunding":{"allTime":"-305.852735","sinceOpen":"2.6037","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"PURR","szi":"-552200.0","leverage":{"type":"cross","value":3},"entryPx":"0.069135","positionValue":"34082.3362","unrealizedPnl":"4094.36687","returnOnEquity":"0.3217433571","liquidationPx":"12.3275383017","marginUsed":"11360.778733","maxLeverage":3,"cumFunding":{"allTime":"-32307.633703","sinceOpen":"-2092.213336","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"HYPE","szi":"-149078.45","leverage":{"type":"cross","value":5},"entryPx":"25.4825","positionValue":"3878574.0336500001","unrealizedPnl":"-79672.19014","returnOnEquity":"-0.1048621331","liquidationPx":"76.4988794996","marginUsed":"775714.80673","maxLeverage":10,"cumFunding":{"allTime":"-309555.435116","sinceOpen":"-3164.915837","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"VIRTUAL","szi":"-9594.1","leverage":{"type":"cross","value":5},"entryPx":"1.92458","positionValue":"10004.72748","unrealizedPnl":"8459.899945","returnOnEquity":"2.2908396011","liquidationPx":"749.8030102371","marginUsed":"2000.945496","maxLeverage":5,"cumFunding":{"allTime":"-818.537548","sinceOpen":"-885.85754","sinceChange":"-132.426133"}}},{"type":"oneWay","position":{"coin":"MORPHO","szi":"-1286.7","leverage":{"type":"cross","value":5},"entryPx":"1.3869","positionValue":"1801.50867","unrealizedPnl":"-16.972812","returnOnEquity":"-0.0475552562","liquidationPx":"5584.4267052968","marginUsed":"360.301734","maxLeverage":5,"cumFunding":{"allTime":"-140.852999","sinceOpen":"-0.524002","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"IP","szi":"55968.6","leverage":{"type":"cross","value":3},"entryPx":"3.75896","positionValue":"211180.72152","unrealizedPnl":"796.732292","returnOnEquity":"0.0113611159","liquidationPx":null,"marginUsed":"70393.57384","maxLeverage":3,"cumFunding":{"allTime":"-975.559391","sinceOpen":"-40.161499","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"MON","szi":"-1114261.0","leverage":{"type":"isolated","value":3,"rawUsd":"36359.245859"},"entryPx":"0.024464","positionValue":"26961.773417","unrealizedPnl":"297.787566","returnOnEquity":"0.0327724536","liquidationPx":"0.0296643783","marginUsed":"9397.472442","maxLeverage":5,"cumFunding":{"allTime":"-574.970969","sinceOpen":"-2.49958","sinceChange":"0.0"}}},{"type":"oneWay","position":{"coin":"MET","szi":"-43463.0","leverage":{"type":"isolated","value":3,"rawUsd":"16030.897561"},"entryPx":"0.27653","positionValue":"11878.87253","unrealizedPnl":"139.95366","returnOnEquity":"0.0349336094","liquidationPx":"0.316148663","marginUsed":"4152.025031","maxLeverage":3,"cumFunding":{"allTime":"-312.089456","sinceOpen":"-1.465492","sinceChange":"0.0"}}}],"time":1768397010203}"#;

        let state: ClearinghouseState = serde_json::from_str(json).unwrap();

        // Check margin summary
        assert_eq!(
            state.margin_summary.account_value.to_string(),
            "8272576.5729350001"
        );
        assert_eq!(
            state.margin_summary.total_margin_used.to_string(),
            "1120386.813659"
        );

        // Check withdrawable
        assert_eq!(state.withdrawable.to_string(), "6286581.8806220004");

        // Check positions count
        assert_eq!(state.asset_positions.len(), 16);

        // Check first position (BTC short)
        let btc_pos = &state.asset_positions[0].position;
        assert_eq!(btc_pos.coin, "BTC");
        assert_eq!(btc_pos.szi.to_string(), "-1.47472");
        assert!(btc_pos.is_short());
        assert_eq!(btc_pos.entry_px.unwrap().to_string(), "95137.8");
        assert_eq!(btc_pos.leverage.value, 20);
        assert!(btc_pos.leverage.is_cross());
        assert_eq!(btc_pos.cum_funding.all_time.to_string(), "-179748.281779");

        // Check a long position (SOL)
        let sol_pos = &state.asset_positions[2].position;
        assert_eq!(sol_pos.coin, "SOL");
        assert!(sol_pos.is_long());
        assert_eq!(sol_pos.szi.to_string(), "30390.93");

        // Check an isolated margin position (MON)
        let mon_pos = &state.asset_positions[14].position;
        assert_eq!(mon_pos.coin, "MON");
        assert!(mon_pos.leverage.is_isolated());
        assert_eq!(mon_pos.leverage.value, 3);
        assert!(mon_pos.leverage.raw_usd.is_some());

        // Check timestamp
        assert_eq!(state.time, 1768397010203);
    }
}
