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
//! All actions that modify state require EIP-712 signatures. The signing domains are:
//! - [`CORE_MAINNET_EIP712_DOMAIN`]: For L1 transactions
//! - [`ARBITRUM_MAINNET_EIP712_DOMAIN`]: For bridging operations
//!
//! # Example: Placing an Order
//!
//! ```rust
//! use hypersdk::hypercore::types::{
//!     OrderRequest, OrderTypePlacement, TimeInForce, Side
//! };
//! use rust_decimal::dec;
//!
//! let order = OrderRequest {
//!     asset: 0,  // BTC
//!     is_buy: true,
//!     limit_px: dec!(50000),  // $50k
//!     sz: dec!(0.1),  // 0.1 BTC
//!     reduce_only: false,
//!     order_type: OrderTypePlacement::Limit {
//!         tif: TimeInForce::Gtc,
//!     },
//!     cloid: [0u8; 16],  // Client order ID
//! };
//! ```
//!
//! # Example: WebSocket Subscription
//!
//! ```rust
//! use hypersdk::hypercore::types::{Subscription, Outgoing};
//!
//! // Subscribe to BTC trades
//! let msg = Outgoing::Subscribe {
//!     subscription: Subscription::Trades {
//!         coin: "BTC".to_string()
//!     }
//! };
//! ```

use std::{collections::HashMap, fmt};

use alloy::{
    dyn_abi::{Eip712Domain, Eip712Types, Resolver, TypedData},
    primitives::{Address, B128, B256, U256, keccak256},
    signers::k256::ecdsa::RecoveryId,
    sol_types::{SolStruct, eip712_domain},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};

use crate::hypercore::{Cloid, OidOrCloid, SpotToken};

const HYPERLIQUID_EIP_PREFIX: &str = "HyperliquidTransaction:";

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
pub(super) const ARBITRUM_MAINNET_EIP712_DOMAIN: Eip712Domain = eip712_domain! {
    name: "HyperliquidSignTransaction",
    version: "1",
    chain_id: 42161,
    verifying_contract: Address::ZERO,
};

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

/// Subscription message.
///
/// Each variant corresponds to a subscription type that can be requested from the WebSocket API.
///
/// # Market Data Subscriptions
///
/// - **Bbo**: Best bid and offer updates for a market
/// - **Trades**: Real-time trade feed for a market
/// - **L2Book**: Order book snapshots and updates (Level 2 depth)
/// - **AllMids**: Mid prices for all markets (optionally filtered by DEX)
///
/// # User-Specific Subscriptions
///
/// - **OrderUpdates**: Real-time updates for user's orders (status changes)
/// - **UserFills**: Real-time fills for user's orders (executions)
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::types::Subscription;
/// use hypersdk::Address;
///
/// // Subscribe to BTC best bid/offer
/// let bbo_sub = Subscription::Bbo {
///     coin: "BTC".to_string()
/// };
///
/// // Subscribe to ETH trades
/// let trades_sub = Subscription::Trades {
///     coin: "ETH".to_string()
/// };
///
/// // Subscribe to order updates for your address
/// let user_addr: Address = "0x...".parse().unwrap();
/// let orders_sub = Subscription::OrderUpdates {
///     user: user_addr
/// };
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
    pub order: BasicOrder,
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

/// Request for an action.
///
/// Contains the action, a nonce, signature, optional vault address, and optional expiry.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ActionRequest {
    /// Action.
    pub action: Action,
    /// Nonce of the message.
    pub nonce: u64,
    /// Signature
    pub signature: Signature,
    /// Trading on behalf of
    pub vault_address: Option<Address>,
    /// Timestamp in milliseconds
    pub expires_after: Option<u64>,
}

/// API response wrapper.
///
/// The `Ok` variant contains a successful response, while `Err` holds an error message.
#[derive(Debug, Deserialize)]
#[serde(tag = "status", content = "response")]
#[serde(rename_all = "camelCase")]
pub enum ApiResponse {
    Ok(OkResponse),
    Err(String),
}

/// Successful API response data.
///
/// Currently supports order responses and a default placeholder.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "camelCase")]
pub enum OkResponse {
    Order { statuses: Vec<OrderResponseStatus> },
    // should be ok?
    Default,
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

/// Signature.
///
/// Represents an EIP‑712 signature split into its components.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Signature {
    pub r: U256,
    pub s: U256,
    pub v: u64,
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

/// Batch order.
///
/// A collection of orders sent together, optionally grouped.
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchOrder {
    pub orders: Vec<OrderRequest>,
    pub grouping: OrderGrouping,
}

/// Order grouping strategy.
///
/// Determines how orders are grouped when sent in a batch.
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum OrderGrouping {
    Na,
    NormalTpsl,
    PositionTpsl,
}

/// Order request.
///
/// Represents a single order within a batch.
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OrderRequest {
    #[serde(rename = "a")]
    pub asset: usize,
    #[serde(rename = "b")]
    pub is_buy: bool,
    #[serde(rename = "p", with = "rust_decimal::serde::str")]
    pub limit_px: Decimal,
    #[serde(rename = "s", with = "rust_decimal::serde::str")]
    pub sz: Decimal,
    #[serde(rename = "r")]
    pub reduce_only: bool,
    #[serde(rename = "t")]
    pub order_type: OrderTypePlacement,
    #[serde(rename = "c")]
    #[serde(with = "const_hex")]
    pub cloid: Cloid,
}

/// Order type for the placement.
///
/// Specifies whether the order is limit or trigger and its associated parameters.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum OrderTypePlacement {
    Limit {
        tif: TimeInForce,
    },
    Trigger {
        #[serde(with = "rust_decimal::serde::str")]
        trigger_px: Decimal,
        is_market: bool,
        tpsl: TpSl,
    },
}

/// Trigger type.
///
/// Indicates whether the trigger is a take‑profit (`Tp`) or stop‑loss (`Sl`).
#[derive(PartialEq, Eq, Deserialize, Serialize, Copy, Clone, Debug)]
#[serde(rename = "lowercase")]
pub enum TpSl {
    Tp,
    Sl,
}

/// Batch modify request.
///
/// Contains a list of order modifications to be applied.
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchModify {
    pub modifies: Vec<Modify>,
}

/// Individual order modification.
///
/// References the order by ID and includes the new order data.
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Modify {
    pub oid: OidOrCloid,
    pub order: OrderRequest,
}

/// Batch cancel request.
///
/// Contains a list of order IDs to cancel.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BatchCancel {
    pub cancels: Vec<Cancel>,
}

/// Batch cancel by cloid request.
///
/// Contains a list of cloid values to cancel.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BatchCancelCloid {
    pub cancels: Vec<CancelByCloid>,
}

/// Cancel request.
///
/// References an order by asset and ID.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Cancel {
    #[serde(rename = "a")]
    pub asset: u32,
    #[serde(rename = "o")]
    pub oid: u64,
}

/// Cancel request by cloid.
///
/// References an order by asset and cloid.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CancelByCloid {
    pub asset: u32,
    #[serde(with = "const_hex")]
    pub cloid: B128,
}

/// Schedule cancellation of all orders.
///
/// The optional `time` field can be used to delay the cancellation.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleCancel {
    pub time: Option<u64>,
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
#[derive(Debug, Clone)]
pub struct SendToken(pub SpotToken);

impl fmt::Display for SendToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{:x}", self.0.name, self.0.token_id)
    }
}

/// Send USDC from the perp balance.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#core-usdc-transfer>
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsdSend {
    /// The chain this action is being executed on.
    pub hyperliquid_chain: HyperliquidChain,
    /// Signature chain ID.
    ///
    /// For arbitrum use [`super::ARBITRUM_SIGNATURE_CHAIN_ID`].
    pub signature_chain_id: &'static str,
    /// The destination address.
    pub destination: Address,
    /// The amount.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    /// Current time, should match the nonce
    pub time: u64,
}

impl UsdSend {
    #[inline(always)]
    pub(super) fn typed_data(&self, msg: &impl Serialize) -> TypedData {
        get_typed_data::<solidity::UsdSend>(msg)
    }
}

/// Send spot tokens.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#core-spot-transfer>
#[serde_as]
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpotSend {
    /// The chain this action is being executed on.
    pub hyperliquid_chain: HyperliquidChain,
    /// Signature chain ID.
    ///
    /// For arbitrum use [`super::ARBITRUM_SIGNATURE_CHAIN_ID`].
    pub signature_chain_id: &'static str,
    /// The destination address.
    pub destination: Address,
    /// Token
    #[serde_as(as = "DisplayFromStr")]
    pub token: SendToken,
    /// The amount.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    /// Current time, should match the nonce
    pub time: u64,
}

impl SpotSend {
    #[inline(always)]
    pub(super) fn typed_data(&self, msg: &impl Serialize) -> TypedData {
        get_typed_data::<solidity::SpotSend>(msg)
    }
}

/// Send asset.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#send-asset>
#[serde_as]
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SendAsset {
    /// The chain this action is being executed on.
    pub hyperliquid_chain: HyperliquidChain,
    /// Signature chain ID.
    ///
    /// For arbitrum use [`super::ARBITRUM_SIGNATURE_CHAIN_ID`].
    pub signature_chain_id: &'static str,
    /// The destination address.
    pub destination: Address,
    /// Source DEX, can be empty
    pub source_dex: String,
    /// Destiation DEX, can be empty
    pub destination_dex: String,
    /// Token
    #[serde_as(as = "DisplayFromStr")]
    pub token: SendToken,
    /// From subaccount, can be empty
    pub from_sub_account: String,
    /// The amount.
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    /// Request nonce
    pub nonce: u64,
}

impl SendAsset {
    #[inline(always)]
    pub(super) fn typed_data(&self, msg: &impl Serialize) -> TypedData {
        get_typed_data::<solidity::SendAsset>(msg)
    }
}

/// Chain for Hyperliquid transactions.
///
/// Indicates whether the transaction is on mainnet or testnet.
#[derive(Serialize, Debug, Copy, Clone, derive_more::Display)]
#[serde(rename_all = "PascalCase")]
pub enum HyperliquidChain {
    #[display("Mainnet")]
    Mainnet,
    #[display("Testnet")]
    Testnet,
}

/// Multi-signature action payload.
///
/// Contains the multisig user address, outer signer, and the inner action to execute.
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MultiSigPayload {
    /// The multisig account address
    pub multi_sig_user: Address,
    /// The address executing the multisig action
    pub outer_signer: Address,
    /// The inner action to execute
    pub action: Box<Action>,
}

/// Multi-signature action wrapper.
///
/// Wraps any action with multiple signatures for multisig execution.
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MultiSigAction {
    /// Signature chain ID (0x66eee for L1 multisig)
    pub signature_chain_id: String,
    /// Signatures from authorized signers
    pub signatures: Vec<Signature>,
    /// The multisig payload
    pub payload: MultiSigPayload,
}

/// An action that requires signing.
///
/// Represents a request to the exchange that must be signed by the user.
#[derive(Clone, Serialize, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum Action {
    /// Order insertion.
    Order(BatchOrder),
    /// Order modification.
    BatchModify(BatchModify),
    /// Order cancellation by oid.
    Cancel(BatchCancel),
    /// Order cancellation by cloid.
    CancelByCloid(BatchCancelCloid),
    /// Schedule cancellation of all orders.
    ScheduleCancel(ScheduleCancel),
    /// Core USDC transfer.
    UsdSend(UsdSend),
    /// Send asset.
    SendAsset(SendAsset),
    /// Spot send.
    SpotSend(SpotSend),
    /// EVM user modify.
    EvmUserModify { using_big_blocks: bool },
    /// Multi-sig action.
    MultiSig(MultiSigAction),
    /// Invalidate a request.
    Noop,
}

impl Action {
    /// Hash the action for signing.
    ///
    /// The hash is generated by serializing the action to MessagePack, appending the nonce,
    /// optional vault address, and optional expiry, then Keccak256 hashing.
    #[inline]
    pub fn hash(
        &self,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        maybe_expires_after: Option<u64>,
    ) -> Result<B256, rmp_serde::encode::Error> {
        rmp_hash(self, nonce, maybe_vault_address, maybe_expires_after)
    }
}

pub(super) fn rmp_hash<T: Serialize>(
    value: &T,
    nonce: u64,
    maybe_vault_address: Option<Address>,
    maybe_expires_after: Option<u64>,
) -> Result<B256, rmp_serde::encode::Error> {
    let mut bytes = rmp_serde::to_vec_named(value)?;
    bytes.extend(nonce.to_be_bytes());

    if let Some(vault_address) = maybe_vault_address {
        bytes.push(1);
        bytes.extend(vault_address.as_slice());
    } else {
        bytes.push(0);
    }

    if let Some(expires_after) = maybe_expires_after {
        bytes.push(0);
        bytes.extend(expires_after.to_be_bytes());
    }

    let signature = keccak256(bytes);
    Ok(B256::from(signature))
}

pub(super) mod solidity {
    use alloy::sol;

    sol! {
        struct Agent {
            string source;
            bytes32 connectionId;
        }

        struct UsdSend {
            string hyperliquidChain;
            string destination;
            string amount;
            uint64 time;
        }

        struct SpotSend {
            string hyperliquidChain;
            string destination;
            string token;
            string amount;
            uint64 time;
        }

        struct SendAsset {
            string hyperliquidChain;
            string destination;
            string sourceDex;
            string destinationDex;
            string token;
            string amount;
            string fromSubAccount;
            uint64 nonce;
        }

        #[derive(serde::Serialize)]
        struct MultiSigEnvelope {
            string hyperliquidChain;
            bytes32 multiSigActionHash;
            uint64 nonce;
        }
    }
}

/// Returns the EIP712 domain and EIP712 types of the `T` message.
///
/// The returned `TypedData` can be used to sign the message with an Ethereum signer.
pub(super) fn get_typed_data<T: SolStruct>(msg: &impl Serialize) -> TypedData {
    let mut resolver = Resolver::from_struct::<T>();
    resolver
        .ingest_string(T::eip712_encode_type())
        .expect("agent");

    let mut types = Eip712Types::from(&resolver);
    let agent_type = types.remove(T::NAME).unwrap();
    types.insert(format!("{HYPERLIQUID_EIP_PREFIX}{}", T::NAME), agent_type);

    TypedData {
        domain: ARBITRUM_MAINNET_EIP712_DOMAIN,
        resolver: Resolver::from(types),
        primary_type: format!("{HYPERLIQUID_EIP_PREFIX}{}", T::NAME),
        message: serde_json::to_value(msg).unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let res = serde_json::from_str::<ApiResponse>(text);
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
        let res = serde_json::from_str::<ApiResponse>(text);
        assert!(res.is_ok());
    }
}
