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
//! - [`UserEvent`]: Funding, liquidation, and non-user-cancel events
//! - [`ActiveAssetData`]: User leverage and trade-size limits
//! - [`FastAssetCtx`]: Low-latency mark/mid price updates
//! - [`UserTwapSliceFills`]: TWAP slice fills for a user
//! - [`UserTwapHistory`]: TWAP lifecycle updates for a user
//!
//! ## Transfer Types
//! - [`UsdSend`]: Send USDC from perp balance
//! - [`SpotSend`]: Send spot tokens
//! - [`SendAsset`]: Send assets between accounts/DEXes
//! - [`AgentSendAsset`]: Agent-signed self-transfer across DEXes/subaccounts
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
    io::Read,
    time::Duration,
};

use alloy::{
    dyn_abi::Eip712Domain,
    primitives::{Address, B128, U256},
    signers::k256::ecdsa::RecoveryId,
    sol_types::eip712_domain,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use flate2::read::DeflateDecoder;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error, ser::SerializeMap};
use serde_with::{DisplayFromStr, serde_as};

use crate::hypercore::{Chain, Cloid, OidOrCloid, SpotToken};

pub mod api;
pub(super) mod solidity;

// Re-export important raw types for convenience
pub use api::{
    AbstractionMode, Action, ActionRequest, ApproveBuilderFee, GossipPriorityBid,
    Hip3LiquidatorTransferAction, MultiSigAction, MultiSigPayload, OkResponse, Response,
    TokenDelegateAction, TwapOrderParams, UsdClassTransferAction, UserDexAbstractionAction,
    UserSetAbstractionAction, Withdraw3Action,
};
use api::{AgentSendAssetAction, SendAssetAction, SpotSendAction, UsdSendAction};

fn decimal_from_json_value(value: &serde_json::Value) -> Result<Decimal, String> {
    match value {
        serde_json::Value::String(s) => s
            .parse::<Decimal>()
            .map_err(|e| format!("invalid decimal string `{s}`: {e}")),
        serde_json::Value::Number(n) => n
            .to_string()
            .parse::<Decimal>()
            .map_err(|e| format!("invalid decimal number `{n}`: {e}")),
        _ => Err("expected decimal as string or number".to_string()),
    }
}

fn deserialize_decimal_from_any<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    decimal_from_json_value(&value).map_err(serde::de::Error::custom)
}

fn deserialize_option_decimal_from_any<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => decimal_from_json_value(&value)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

fn deserialize_optional_decimal_pair_from_any<'de, D>(
    deserializer: D,
) -> Result<Option<[Decimal; 2]>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Vec<serde_json::Value>>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(values) => {
            let [left, right]: [serde_json::Value; 2] = values.try_into().map_err(|_| {
                serde::de::Error::custom("expected array with exactly two decimal values")
            })?;
            Ok(Some([
                decimal_from_json_value(&left).map_err(serde::de::Error::custom)?,
                decimal_from_json_value(&right).map_err(serde::de::Error::custom)?,
            ]))
        }
    }
}

fn deserialize_fast_asset_ctxs<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, FastAssetCtx>, D::Error>
where
    D: Deserializer<'de>,
{
    let encoded = String::deserialize(deserializer)?;
    let compressed = BASE64_STANDARD
        .decode(encoded)
        .map_err(serde::de::Error::custom)?;
    let mut decoder = DeflateDecoder::new(compressed.as_slice());
    let mut json = String::new();
    decoder
        .read_to_string(&mut json)
        .map_err(serde::de::Error::custom)?;
    serde_json::from_str(&json).map_err(serde::de::Error::custom)
}

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
    pub(super) deployer_fee_scale: Option<Decimal>,
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
        Dex {
            name,
            index,
            deployer_fee_scale: None,
        }
    }

    /// Returns the DEX name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the DEX index.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the deployer fee scale for this DEX.
    #[must_use]
    pub fn deployer_fee_scale(&self) -> Option<Decimal> {
        self.deployer_fee_scale
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
/// | [`FastAssetCtxs`](Self::FastAssetCtxs) | [`Incoming::FastAssetCtxs`] | Low-latency mark/mid price updates |
///
/// # User-Specific Subscriptions
///
/// | Subscription | Incoming Message | Description |
/// |--------------|------------------|-------------|
/// | [`OrderUpdates`](Self::OrderUpdates) | [`Incoming::OrderUpdates`] | Order status changes |
/// | [`UserFills`](Self::UserFills) | [`Incoming::UserFills`] | Trade fills |
/// | [`UserEvents`](Self::UserEvents) | [`Incoming::UserEvents`] | Funding, liquidation, and non-user-cancel updates |
/// | [`UserTwapSliceFills`](Self::UserTwapSliceFills) | [`Incoming::UserTwapSliceFills`] | TWAP slice fill updates |
/// | [`UserTwapHistory`](Self::UserTwapHistory) | [`Incoming::UserTwapHistory`] | TWAP lifecycle history updates |
/// | [`ActiveAssetData`](Self::ActiveAssetData) | [`Incoming::ActiveAssetData`] | User leverage and trading limits for a perp asset |
/// | [`WebData2`](Self::WebData2) | [`Incoming::WebData2`] | Frontend-style aggregate account snapshot |
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
    L2Book {
        coin: String,
        /// Aggregate price levels to this many significant figures (valid: 2-5; `None` for full precision).
        #[serde(default, rename = "nSigFigs", skip_serializing_if = "Option::is_none")]
        n_sig_figs: Option<u8>,
        /// Further aggregation; only valid when `n_sig_figs` is `5` (values: 1, 2, or 5).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mantissa: Option<u8>,
        /// Opt into Hyperliquid's faster l2Book mode introduced with the websocket push-frequency
        /// migration: `fast: true` pushes 5 levels roughly every 0.5s, while the default feed
        /// remains the deeper, slower 20-level snapshot stream.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        fast: bool,
    },
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
    /// User events (funding, liquidation, non-user-cancel)
    #[display("userEvents({user})")]
    UserEvents { user: Address },
    /// TWAP slice fill updates for user
    #[display("userTwapSliceFills({user})")]
    UserTwapSliceFills { user: Address },
    /// TWAP history updates for user
    #[display("userTwapHistory({user})")]
    UserTwapHistory { user: Address },
    /// Real-time asset context (funding rate, mark price, open interest)
    #[display("activeAssetCtx({coin})")]
    ActiveAssetCtx { coin: String },
    /// User-specific asset limits and leverage information (perps only)
    #[display("activeAssetData({user},{coin})")]
    ActiveAssetData { user: Address, coin: String },
    /// Frontend-oriented aggregate user data feed
    #[display("webData2({user},{dex:?})")]
    WebData2 {
        user: Address,
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    #[display("clearinghouseState({user},{dex:?})")]
    ClearinghouseState {
        user: Address,
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    #[display("allDexsClearinghouseState({user})")]
    AllDexsClearinghouseState { user: Address },
    #[display("openOrders({user},{dex:?})")]
    OpenOrders {
        user: Address,
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    #[display("spotState({user},{is_portfolio_margin:?})")]
    SpotState {
        user: Address,
        #[serde(
            default,
            rename = "isPortfolioMargin",
            skip_serializing_if = "Option::is_none"
        )]
        is_portfolio_margin: Option<bool>,
    },
    /// User notifications
    #[display("notification({user})")]
    Notification { user: Address },
    /// Frontend-oriented aggregate user data feed (v3, replaces WebData2)
    #[display("webData3({user})")]
    WebData3 { user: Address },
    /// Active TWAP order states
    #[display("twapStates({user},{dex:?})")]
    TwapStates {
        user: Address,
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    /// Real-time funding updates
    #[display("userFundings({user})")]
    UserFundings { user: Address },
    /// Non-funding ledger events
    #[display("userNonFundingLedgerUpdates({user})")]
    UserNonFundingLedgerUpdates { user: Address },
    /// Asset contexts across all DEXs
    #[display("allDexsAssetCtxs")]
    AllDexsAssetCtxs,
    /// Low-latency mark/mid price updates for all assets
    #[display("fastAssetCtxs")]
    FastAssetCtxs,
    /// Outcome market metadata updates
    #[display("outcomeMetaUpdates")]
    OutcomeMetaUpdates,
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
/// - **UserEvents**: Funding/liquidation/non-user-cancel events for a user
/// - **UserTwapSliceFills**: TWAP slice fill updates for a user
/// - **UserTwapHistory**: TWAP status history updates for a user
/// - **ActiveAssetData**: User leverage and limits for a specific perp asset
/// - **FastAssetCtxs**: Low-latency mark/mid price updates for all assets
/// - **WebData2**: Frontend-style aggregate user snapshot
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
    OrderUpdates(Vec<OrderUpdate<WsBasicOrder>>),
    /// Fill events for a user
    #[serde(rename_all = "camelCase")]
    UserFills {
        #[serde(default)]
        is_snapshot: bool,
        user: Address,
        fills: Vec<Fill>,
    },
    /// User events for a user (fills, funding, liquidation, non-user-cancel).
    /// Hyperliquid may send fill notifications on channel `"user"` instead of `"userEvents"`.
    #[serde(alias = "user")]
    UserEvents(UserEvent),
    /// TWAP slice fill updates for a user
    UserTwapSliceFills(UserTwapSliceFills),
    /// TWAP history updates for a user
    UserTwapHistory(UserTwapHistory),
    /// Real-time asset context update (funding rate, mark price, etc.)
    ActiveAssetCtx { coin: String, ctx: AssetContext },
    /// Real-time spot asset context update (funding rate, mark price, etc.)
    ActiveSpotAssetCtx { coin: String, ctx: SpotAssetContext },
    /// Real-time user asset limits/leverage for a perp asset
    ActiveAssetData(ActiveAssetData),
    /// Frontend aggregate user snapshot (dynamic schema)
    WebData2 {
        dex: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    /// Clearing house state for a user on a specific dex
    #[serde(rename_all = "camelCase")]
    ClearinghouseState {
        dex: Option<String>,
        user: Address,
        clearinghouse_state: ClearinghouseState,
    },
    /// Clearing house state for a user on a all dexs
    #[serde(rename_all = "camelCase")]
    AllDexsClearinghouseState {
        user: Address,
        clearinghouse_states: Vec<(String, ClearinghouseState)>,
    },
    /// Open orders for a user on a specific dex
    OpenOrders {
        dex: Option<String>,
        user: Address,
        orders: Vec<OpenOrder>,
    },
    /// Spot state update
    #[serde(rename_all = "camelCase")]
    SpotState {
        user: Address,
        spot_state: SpotState,
    },
    /// User notification
    Notification { notification: String },
    /// Frontend aggregate user snapshot v3 (dynamic schema)
    WebData3 {
        #[serde(flatten)]
        data: serde_json::Value,
    },
    /// Active TWAP order states
    #[serde(rename_all = "camelCase")]
    TwapStates {
        dex: Option<String>,
        user: Address,
        states: Vec<(u64, serde_json::Value)>,
    },
    /// Real-time user funding updates
    #[serde(rename_all = "camelCase")]
    UserFundings {
        #[serde(default)]
        is_snapshot: bool,
        user: Address,
        fundings: Vec<UserFundingEntry>,
    },
    /// Non-funding ledger updates
    #[serde(rename_all = "camelCase")]
    UserNonFundingLedgerUpdates {
        #[serde(default)]
        is_snapshot: bool,
        user: Address,
        updates: Vec<serde_json::Value>,
    },
    /// Asset contexts across all DEXs
    AllDexsAssetCtxs {
        ctxs: Vec<(String, Vec<PerpAssetCtx>)>,
    },
    /// Low-latency mark/mid price updates for all assets.
    ///
    /// Hyperliquid sends this channel as base64-encoded raw-DEFLATE JSON. The SDK
    /// decodes it before exposing the map.
    FastAssetCtxs(
        #[serde(deserialize_with = "deserialize_fast_asset_ctxs")] HashMap<String, FastAssetCtx>,
    ),
    /// Outcome market metadata updates
    OutcomeMetaUpdates(serde_json::Value),
    /// Server heartbeat ping
    Ping,
    /// Server heartbeat pong
    Pong,
}

/// WebSocket order update.
///
/// Contains status, timestamp, and the original order details.
/// Type parameter `T` can be [`WsBasicOrder`] or [`BasicOrder`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderUpdate<T> {
    pub status: OrderStatus,
    pub status_timestamp: u64,
    pub order: T,
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
    /// Participant addresses: [buyer, seller]
    #[serde(default)]
    pub users: [Address; 2],
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

    /// Returns the taker's wallet address.
    ///
    /// `users` is `[buyer, seller]`. The taker is the buyer on a `Bid`
    /// and the seller on an `Ask`.
    #[must_use]
    pub fn taker_address(&self) -> Address {
        match self.side {
            Side::Bid => self.users[0],
            Side::Ask => self.users[1],
        }
    }

    /// Returns the maker's wallet address.
    ///
    /// `users` is `[buyer, seller]`. The maker is the seller on a `Bid`
    /// and the buyer on an `Ask`.
    #[must_use]
    pub fn maker_address(&self) -> Address {
        match self.side {
            Side::Bid => self.users[1],
            Side::Ask => self.users[0],
        }
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
    /// use [`Self::to_duration_with_month_days`] instead.
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
    /// use hypersdk::hypercore::CandleInterval;
    ///
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
    pub snapshot: bool,
    /// [bids, asks]
    pub levels: [Vec<BookLevel>; 2],
}

impl L2Book {
    /// Returns true if this is a full snapshot (not a delta update).
    #[must_use]
    pub fn is_snapshot(&self) -> bool {
        self.snapshot
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

/// Direction of a user fill.
///
/// These values are serialized and deserialized using Hyperliquid's wire strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display)]
pub enum FillDirection {
    /// Opening a long position.
    #[serde(rename = "Open Long")]
    #[display("Open Long")]
    OpenLong,
    /// Opening a short position.
    #[serde(rename = "Open Short")]
    #[display("Open Short")]
    OpenShort,
    /// Closing a long position.
    #[serde(rename = "Close Long")]
    #[display("Close Long")]
    CloseLong,
    /// Closing a short position.
    #[serde(rename = "Close Short")]
    #[display("Close Short")]
    CloseShort,
    /// Flipping from long to short.
    #[serde(rename = "Long > Short")]
    #[display("Long > Short")]
    LongToShort,
    /// Flipping from short to long.
    #[serde(rename = "Short > Long")]
    #[display("Short > Long")]
    ShortToLong,
    /// Cross-margin long liquidation.
    #[serde(rename = "Liquidated Cross Long")]
    #[display("Liquidated Cross Long")]
    LiquidatedCrossLong,
    /// Cross-margin short liquidation.
    #[serde(rename = "Liquidated Cross Short")]
    #[display("Liquidated Cross Short")]
    LiquidatedCrossShort,
    /// Isolated-margin long liquidation.
    #[serde(rename = "Liquidated Isolated Long")]
    #[display("Liquidated Isolated Long")]
    LiquidatedIsolatedLong,
    /// Isolated-margin short liquidation.
    #[serde(rename = "Liquidated Isolated Short")]
    #[display("Liquidated Isolated Short")]
    LiquidatedIsolatedShort,
    /// Auto-deleveraging event.
    #[serde(rename = "Auto-Deleveraging")]
    #[display("Auto-Deleveraging")]
    AutoDeleveraging,
    /// Partial borrow liquidation.
    #[serde(rename = "Partial Borrow Liquidation")]
    #[display("Partial Borrow Liquidation")]
    PartialBorrowLiquidation,
    /// Backstop borrow liquidation.
    #[serde(rename = "Backstop Borrow Liquidation")]
    #[display("Backstop Borrow Liquidation")]
    BackstopBorrowLiquidation,
    /// Settlement.
    #[serde(rename = "Settlement")]
    #[display("Settlement")]
    Settlement,
    /// Net child vault position change.
    #[serde(rename = "Net Child Vaults")]
    #[display("Net Child Vaults")]
    NetChildVaults,
    /// Spot buy.
    #[serde(rename = "Buy")]
    #[display("Buy")]
    Buy,
    /// Spot sell.
    #[serde(rename = "Sell")]
    #[display("Sell")]
    Sell,
    /// Automatic spot dust conversion.
    #[serde(rename = "Spot Dust Conversion")]
    #[display("Spot Dust Conversion")]
    SpotDustConversion,
}

impl FillDirection {
    /// Returns the Hyperliquid wire string for this fill direction.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::OpenLong => "Open Long",
            Self::OpenShort => "Open Short",
            Self::CloseLong => "Close Long",
            Self::CloseShort => "Close Short",
            Self::LongToShort => "Long > Short",
            Self::ShortToLong => "Short > Long",
            Self::LiquidatedCrossLong => "Liquidated Cross Long",
            Self::LiquidatedCrossShort => "Liquidated Cross Short",
            Self::LiquidatedIsolatedLong => "Liquidated Isolated Long",
            Self::LiquidatedIsolatedShort => "Liquidated Isolated Short",
            Self::AutoDeleveraging => "Auto-Deleveraging",
            Self::PartialBorrowLiquidation => "Partial Borrow Liquidation",
            Self::BackstopBorrowLiquidation => "Backstop Borrow Liquidation",
            Self::Settlement => "Settlement",
            Self::NetChildVaults => "Net Child Vaults",
            Self::Buy => "Buy",
            Self::Sell => "Sell",
            Self::SpotDustConversion => "Spot Dust Conversion",
        }
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
/// - `dir`: Fill direction
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
    /// Fill direction
    pub dir: FillDirection,
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
    /// Builder fee amount, if a builder was used
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builder_fee: Option<Decimal>,
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

/// User event payload for `userEvents` subscription.
///
/// Hyperliquid sends a single-key object where the key identifies the event class.
/// This enum models the documented event classes and preserves unknown payloads.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UserEvent {
    /// Array of trade fills.
    Fills { fills: Vec<Fill> },
    /// Funding payment event.
    Funding { funding: UserFunding },
    /// Liquidation event.
    Liquidation { liquidation: UserLiquidation },
    /// Non-user cancellation event.
    NonUserCancel {
        #[serde(rename = "nonUserCancel")]
        non_user_cancel: Vec<NonUserCancel>,
    },
    /// Unknown user event payload (forward-compatible fallback).
    Unknown(serde_json::Value),
}

/// Funding payment event in `userEvents`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFunding {
    pub time: u64,
    pub coin: String,
    #[serde(deserialize_with = "deserialize_decimal_from_any")]
    pub usdc: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_any")]
    pub szi: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_any")]
    pub funding_rate: Decimal,
}

/// Liquidation event in `userEvents`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UserLiquidation {
    pub lid: u64,
    pub liquidator: Address,
    pub liquidated_user: Address,
    #[serde(deserialize_with = "deserialize_decimal_from_any")]
    pub liquidated_ntl_pos: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_any")]
    pub liquidated_account_value: Decimal,
}

/// Cancellation record in `userEvents.nonUserCancel`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NonUserCancel {
    pub coin: String,
    pub oid: u64,
}

/// User leverage information for `activeAssetData`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLeverage {
    #[serde(rename = "type")]
    pub leverage_type: String,
    #[serde(deserialize_with = "deserialize_decimal_from_any")]
    pub value: Decimal,
    #[serde(default)]
    pub raw_usd: Option<Decimal>,
}

/// `activeAssetData` feed payload.
///
/// Includes user leverage configuration and per-side trade size availability.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAssetData {
    pub user: Address,
    pub coin: String,
    pub leverage: UserLeverage,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_pair_from_any"
    )]
    pub max_trade_szs: Option<[Decimal; 2]>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_decimal_pair_from_any"
    )]
    pub available_to_trade: Option<[Decimal; 2]>,
    #[serde(default)]
    pub mark_px: Option<Decimal>,
}

impl ActiveAssetData {
    /// Returns the max tradable size for buy and sell directions, if provided.
    #[must_use]
    pub fn max_trade_szs_pair(&self) -> Option<(Decimal, Decimal)> {
        self.max_trade_szs.map(|pair| (pair[0], pair[1]))
    }

    /// Returns available tradable size for buy and sell directions, if provided.
    #[must_use]
    pub fn available_to_trade_pair(&self) -> Option<(Decimal, Decimal)> {
        self.available_to_trade.map(|pair| (pair[0], pair[1]))
    }
}

/// One TWAP slice fill entry from `userTwapSliceFills`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwapSliceFill {
    pub fill: Fill,
    pub twap_id: u64,
}

/// `userTwapSliceFills` feed payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserTwapSliceFills {
    #[serde(default)]
    pub is_snapshot: bool,
    pub user: Address,
    #[serde(default)]
    pub twap_slice_fills: Vec<TwapSliceFill>,
}

/// TWAP status enum for `userTwapHistory`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TwapStatus {
    Activated,
    Terminated,
    Finished,
    Error,
    #[serde(other)]
    Unknown,
}

/// TWAP status payload from `userTwapHistory`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwapHistoryStatus {
    pub status: TwapStatus,
    #[serde(default)]
    pub description: Option<String>,
}

/// TWAP state payload from `userTwapHistory`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwapState {
    pub coin: String,
    pub user: Address,
    pub side: String,
    #[serde(deserialize_with = "deserialize_decimal_from_any")]
    pub sz: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_any")]
    pub executed_sz: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_any")]
    pub executed_ntl: Decimal,
    pub minutes: u64,
    pub reduce_only: bool,
    pub randomize: bool,
    pub timestamp: u64,
}

/// One TWAP history entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwapHistory {
    pub state: TwapState,
    pub status: TwapHistoryStatus,
    pub time: u64,
}

/// `userTwapHistory` feed payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserTwapHistory {
    #[serde(default)]
    pub is_snapshot: bool,
    pub user: Address,
    #[serde(default)]
    pub history: Vec<TwapHistory>,
}

/// Order details.
///
/// Basic information needed for creating or updating an order.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde_as]
#[serde(rename_all = "camelCase")]
pub struct BasicOrder {
    /// Unix timestamp (ms) when the order was placed.
    pub timestamp: u64,
    /// Coin/market symbol (e.g., "BTC").
    pub coin: String,
    /// Buy or sell side.
    pub side: Side,
    /// Limit price.
    pub limit_px: Decimal,
    /// Remaining size to fill.
    pub sz: Decimal,
    /// Exchange-assigned order ID.
    pub oid: u64,
    /// Original size at placement.
    pub orig_sz: Decimal,
    /// Client-assigned order ID (if set).
    pub cloid: Option<B128>,
    /// Order type (limit, market, etc.).
    pub order_type: OrderType,
    /// Time-in-force (GTC, IOC, ALO).
    pub tif: Option<TimeInForce>,
    /// Whether this order should only reduce an existing position.
    pub reduce_only: bool,
    /// Whether this is a trigger (stop/take-profit) order (`frontendOpenOrders` only).
    #[serde(default)]
    pub is_trigger: Option<bool>,
    /// Trigger price for stop/take-profit orders (`frontendOpenOrders` only).
    #[serde(default)]
    pub trigger_px: Option<Decimal>,
    /// Trigger condition string, e.g. "Price above 10.0" or "N/A" (`frontendOpenOrders` only).
    #[serde(default)]
    pub trigger_condition: Option<String>,
    /// Whether the order is part of a position-level TP/SL bracket (`frontendOpenOrders` only).
    #[serde(default)]
    pub is_position_tpsl: Option<bool>,
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
    /// Unix timestamp (ms) when the order was placed.
    pub timestamp: u64,
    /// Coin/market symbol (e.g., "BTC").
    pub coin: String,
    /// Buy or sell side.
    pub side: Side,
    /// Limit price.
    pub limit_px: Decimal,
    /// Remaining size to fill.
    pub sz: Decimal,
    /// Exchange-assigned order ID.
    pub oid: u64,
    /// Original size at placement.
    pub orig_sz: Decimal,
    /// Client-assigned order ID (if set).
    pub cloid: Option<B128>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde_as]
#[serde(rename_all = "camelCase")]
pub struct OpenOrder {
    #[serde(flatten)]
    pub basic_order: BasicOrder,
    pub trigger_condition: String,
    pub is_trigger: bool,
    pub trigger_px: Decimal,
    pub children: Vec<OpenOrder>,
    pub is_position_tpsl: bool,
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
    #[serde(rename = "Stop Market")]
    StopMarket,
    #[serde(rename = "Stop Limit")]
    StopLimit,
    #[serde(rename = "Take Profit Market")]
    TakeProfitMarket,
    #[serde(rename = "Take Profit Limit")]
    TakeProfitLimit,
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
    /// Destination address.
    pub destination: Address,
    /// Amount of USDC to send.
    pub amount: Decimal,
    /// Unix timestamp (ms); doubles as the action nonce.
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

/// Asset target for transfers.
///
/// Specifies whether a transfer destination is a perpetual (perp) balance,
/// a spot balance, or a HIP-3 DEX identified by name.
#[derive(Debug, Clone, derive_more::Display)]
pub enum AssetTarget {
    #[display("")]
    Perp,
    #[display("spot")]
    Spot,
    #[display("{_0}")]
    Dex(String),
}

impl std::str::FromStr for AssetTarget {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "" | "perp" => Self::Perp,
            "spot" => Self::Spot,
            dex => Self::Dex(dex.to_string()),
        })
    }
}

/// Send asset between accounts or DEXes (inner data).
///
/// This is the core data structure for sending assets across different contexts
/// (e.g., between DEXes or subaccounts). To create a signable action,
/// use the `into_action()` method to convert it to a `SendAssetAction`.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#send-asset>
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendAsset {
    /// The destination address.
    pub destination: Address,
    /// Source DEX or balance context (e.g., [`AssetTarget::Perp`], [`AssetTarget::Spot`]).
    #[serde_as(as = "DisplayFromStr")]
    pub source_dex: AssetTarget,
    /// Destination DEX or balance context (e.g., [`AssetTarget::Perp`], [`AssetTarget::Spot`]).
    #[serde_as(as = "DisplayFromStr")]
    pub destination_dex: AssetTarget,
    /// Token to send.
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
            source_dex: self.source_dex.to_string(),
            destination_dex: self.destination_dex.to_string(),
            token: self.token.to_string(),
            amount: self.amount,
            from_sub_account: self.from_sub_account,
            nonce: self.nonce,
        }
    }
}

/// Agent-signed variant of [`SendAsset`] (inner data).
///
/// Similar to [`SendAsset`] but signed by an agent (API wallet) using L1-action
/// signing. The destination is fixed to the source address, so this is
/// restricted to self-transfers across DEXes, the spot balance, or between
/// subaccounts owned by the same master account.
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#agent-send-asset>
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSendAsset {
    /// The destination address (must equal the signer's source address).
    pub destination: Address,
    /// Source DEX.
    #[serde_as(as = "DisplayFromStr")]
    pub source_dex: AssetTarget,
    /// Destination DEX.
    #[serde_as(as = "DisplayFromStr")]
    pub destination_dex: AssetTarget,
    /// Token.
    pub token: SendToken,
    /// Amount to send.
    pub amount: Decimal,
    /// Source subaccount address, or empty string if sending from the main account.
    pub from_sub_account: String,
    /// Request nonce (timestamp in ms); must match the outer request nonce.
    pub nonce: u64,
}

impl AgentSendAsset {
    /// Converts this into a signable [`AgentSendAssetAction`].
    #[must_use]
    pub fn into_action(self) -> AgentSendAssetAction {
        AgentSendAssetAction {
            destination: self.destination,
            source_dex: self.source_dex.to_string(),
            destination_dex: self.destination_dex.to_string(),
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
/// - **WaitingForTrigger**: Trigger order accepted, waiting for its trigger price
/// - **WaitingForFill**: Order accepted, waiting to be filled
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
///     OrderResponseStatus::WaitingForTrigger => {
///         println!("Trigger order waiting for trigger price");
///     }
///     OrderResponseStatus::WaitingForFill => {
///         println!("Order waiting to fill");
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
    /// Trigger order accepted, waiting for its trigger price to be reached
    WaitingForTrigger,
    /// Order accepted, waiting to be filled
    WaitingForFill,
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
/// - **Multiple independent orders**: Set `grouping` to `"na"`
/// - **Bracket orders (TP/SL)**: Use `"normalTpsl"` or `"positionTpsl"`
///
/// # Related Types
///
/// - [`OrderRequest`]: Individual order within the batch
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
///     builder: None,
/// };
/// ```
///
/// # Write Priority Example
///
/// ```no_run
/// use hypersdk::hypercore::types::*;
/// use rust_decimal::dec;
///
/// let prioritized = BatchOrder {
///     orders: vec![
///         OrderRequest {
///             asset: 0, // BTC
///             is_buy: true,
///             limit_px: dec!(50000),
///             sz: dec!(0.1),
///             reduce_only: false,
///             order_type: OrderTypePlacement::Limit {
///                 tif: TimeInForce::Ioc, // Required for write priority
///             },
///             cloid: Default::default(),
///         }
///     ],
///     grouping: OrderGrouping::PriorityRate(80_000), // 8 bps max
///     builder: None,
/// };
/// ```
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchOrder {
    pub orders: Vec<OrderRequest>,
    pub grouping: OrderGrouping,
    /// Optional builder to receive fees for routed orders.
    ///
    /// User must approve a maximum fee first via [`api::ApproveBuilderFee`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub builder: Option<Builder>,
}

/// Builder fee metadata attached to an order action.
///
/// Serialized under the `builder` key as `{ "b": <address>, "f": <tenths_of_bps> }`.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Builder {
    /// Builder address.
    #[serde(rename = "b")]
    #[serde(
        serialize_with = "crate::hypercore::utils::serialize_address_as_hex",
        deserialize_with = "crate::hypercore::utils::deserialize_address_from_hex"
    )]
    pub builder_address: Address,
    /// Builder fee in tenths of basis points.
    #[serde(rename = "f")]
    pub fee: u32,
}

/// Grouping type for batch orders.
///
/// Serializes as a plain string (`"na"`, `"normalTpsl"`, `"positionTpsl"`) or as an
/// object with a priority rate: `{"p": N}` where N is in units of 1/10_000_000 of
/// filled notional (max 8 bps → `80_000`). All orders must be IOC.
#[derive(Clone, Debug)]
pub enum OrderGrouping {
    Na,
    NormalTpsl,
    PositionTpsl,
    /// Pay a priority tip burned at fill time for faster matching.
    /// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/priority-fees#order-write-priority>
    PriorityRate(u32),
}

impl Serialize for OrderGrouping {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Na => s.serialize_str("na"),
            Self::NormalTpsl => s.serialize_str("normalTpsl"),
            Self::PositionTpsl => s.serialize_str("positionTpsl"),
            Self::PriorityRate(p) => {
                let mut map = s.serialize_map(Some(1))?;
                map.serialize_entry("p", p)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for OrderGrouping {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Str(String),
            Obj { p: u32 },
        }
        match Raw::deserialize(d)? {
            Raw::Str(s) => match s.as_str() {
                "na" => Ok(Self::Na),
                "normalTpsl" => Ok(Self::NormalTpsl),
                "positionTpsl" => Ok(Self::PositionTpsl),
                other => Err(Error::custom(format!("unknown grouping variant: {other}"))),
            },
            Raw::Obj { p } => Ok(Self::PriorityRate(p)),
        }
    }
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
    /// Uses normalized serialization (removes trailing zeros) for consistent hashing.
    #[serde(rename = "p", with = "super::utils::decimal_normalized")]
    pub limit_px: Decimal,
    /// Order size in base asset units.
    /// Uses normalized serialization (removes trailing zeros) for consistent hashing.
    #[serde(rename = "s", with = "super::utils::decimal_normalized")]
    pub sz: Decimal,
    /// When `true`, the order can only reduce an existing position.
    #[serde(rename = "r")]
    pub reduce_only: bool,
    /// Order type specifying limit or trigger parameters.
    #[serde(rename = "t")]
    pub order_type: OrderTypePlacement,
    /// Client-supplied order ID for tracking.
    ///
    /// When set to `Cloid::ZERO` (or `Default`), this field is omitted from serialization
    /// to match the server's hashing behavior (consistent with the Python SDK).
    #[serde(rename = "c")]
    #[serde(
        serialize_with = "super::utils::serialize_cloid_option",
        deserialize_with = "super::utils::deserialize_cloid_option",
        skip_serializing_if = "super::utils::is_cloid_zero",
        default
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
        #[serde(with = "super::utils::decimal_normalized")]
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
    #[serde(with = "super::utils::oid_or_cloid")]
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
/// let state = client.clearinghouse_state(user, None).await?;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display)]
#[serde(rename_all = "camelCase")]
pub enum PositionType {
    /// One-way position mode (single position per market)
    #[display("oneWay")]
    OneWay,
}

/// A user's position in a specific asset.
///
/// Wraps the position details along with cumulative funding information.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, derive_more::Display)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
/// Hyperliquid pays funding every hour.
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
    /// Returns the annualized funding rate.
    #[must_use]
    pub fn annualized_rate(&self) -> Decimal {
        self.funding_rate * Decimal::from(24 * 365)
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

/// Real-time asset context from activeAssetCtx WebSocket subscription.
///
/// Contains live funding rate, open interest, mark/oracle prices, and other
/// real-time market metrics for a perpetual contract.
///
/// # Fields
///
/// - `funding`: Current hourly funding rate
/// - `open_interest`: Total open interest in the market
/// - `mark_px`: Mark price used for liquidations
/// - `oracle_px`: Oracle price from external feed
/// - `mid_px`: Mid price between best bid and ask
/// - `premium`: Premium component of the funding rate
/// - `prev_day_px`: Previous day's closing price
/// - `day_ntl_vlm`: 24h notional volume
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore::{self, ws::Event, types::*};
/// use futures::StreamExt;
///
/// # async fn example() -> anyhow::Result<()> {
/// let mut ws = hypercore::mainnet_ws();
/// ws.subscribe(Subscription::ActiveAssetCtx { coin: "BTC".into() });
///
/// while let Some(event) = ws.next().await {
///     let Event::Message(msg) = event else { continue };
///     if let Incoming::ActiveAssetCtx { coin, ctx } = msg {
///         println!("{} funding: {} ({}% APR)",
///             coin, ctx.funding, ctx.annualized_rate() * rust_decimal::Decimal::from(100));
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetContext {
    /// Current hourly funding rate
    #[serde(with = "rust_decimal::serde::str")]
    pub funding: Decimal,
    /// Total open interest
    #[serde(with = "rust_decimal::serde::str")]
    pub open_interest: Decimal,
    /// Mark price (used for liquidations)
    #[serde(with = "rust_decimal::serde::str_option", default)]
    pub mark_px: Option<Decimal>,
    /// Oracle price from external feed
    #[serde(with = "rust_decimal::serde::str_option", default)]
    pub oracle_px: Option<Decimal>,
    /// Mid price between best bid/ask
    #[serde(with = "rust_decimal::serde::str_option", default)]
    pub mid_px: Option<Decimal>,
    /// Premium component of funding
    #[serde(with = "rust_decimal::serde::str_option", default)]
    pub premium: Option<Decimal>,
    /// Previous day closing price
    #[serde(with = "rust_decimal::serde::str")]
    pub prev_day_px: Decimal,
    /// 24h notional volume
    #[serde(with = "rust_decimal::serde::str")]
    pub day_ntl_vlm: Decimal,
    /// Impact prices [bid, ask] for funding calculation
    #[serde(default)]
    pub impact_pxs: Option<Vec<String>>,
    /// 24h base volume (HIP-3 DEXs only)
    #[serde(with = "rust_decimal::serde::str_option", default)]
    pub day_base_vlm: Option<Decimal>,
}

impl AssetContext {
    /// Returns the annualized funding rate.
    #[must_use]
    pub fn annualized_rate(&self) -> Decimal {
        self.funding * Decimal::from(24 * 365)
    }

    /// Returns true if the funding rate is positive (longs pay shorts).
    #[must_use]
    pub fn is_positive(&self) -> bool {
        self.funding > Decimal::ZERO
    }

    /// Returns true if the funding rate is negative (shorts pay longs).
    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.funding < Decimal::ZERO
    }
}

/// Real-time spot asset context from activeSpotAssetCtx WebSocket subscription.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotAssetContext {
    /// Mark price (used for liquidations)
    #[serde(with = "rust_decimal::serde::str_option", default)]
    pub mark_px: Option<Decimal>,
    /// Mid price between best bid/ask
    #[serde(with = "rust_decimal::serde::str_option", default)]
    pub mid_px: Option<Decimal>,
    /// Previous day closing price
    #[serde(with = "rust_decimal::serde::str")]
    pub prev_day_px: Decimal,
    /// 24h notional quote volume
    #[serde(with = "rust_decimal::serde::str")]
    pub day_ntl_vlm: Decimal,
    /// 24h notional base volume
    #[serde(with = "rust_decimal::serde::str")]
    pub day_base_vlm: Decimal,
}

/// Low-latency asset context from the `fastAssetCtxs` WebSocket subscription.
///
/// The first message is a snapshot. Later messages contain only assets and
/// fields that changed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FastAssetCtx {
    /// Latest mark price when present in the update.
    #[serde(default, deserialize_with = "deserialize_option_decimal_from_any")]
    pub mark_px: Option<Decimal>,
    /// Latest mid price when present. Hyperliquid may explicitly send `null`.
    #[serde(default, deserialize_with = "deserialize_option_decimal_from_any")]
    pub mid_px: Option<Decimal>,
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
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserBalance {
    /// Token symbol
    pub coin: String,
    /// Token index (absent for outcome market balances)
    #[serde(default)]
    pub token: Option<usize>,
    /// Amount held (locked)
    pub hold: Decimal,
    /// Total balance
    pub total: Decimal,
    /// Entry notional
    pub entry_ntl: Decimal,
}

/// User-specific trading fee rates.
///
/// Returned by the `userFees` info endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFees {
    /// Daily user volume breakdown by date.
    pub daily_user_vlm: serde_json::Value,
    /// Fee schedule details.
    pub fee_schedule: serde_json::Value,
    /// Effective perpetual maker fee rate.
    #[serde(rename = "userAddRate")]
    pub maker_rate: Decimal,
    /// Effective perpetual taker fee rate.
    #[serde(rename = "userCrossRate")]
    pub taker_rate: Decimal,
    /// Effective spot maker fee rate.
    #[serde(rename = "userSpotAddRate")]
    pub spot_maker_rate: Decimal,
    /// Effective spot taker fee rate.
    #[serde(rename = "userSpotCrossRate")]
    pub spot_taker_rate: Decimal,
    /// Active referral discount applied to the user.
    pub active_referral_discount: Decimal,
    /// Whether the user is in a fee trial period.
    #[serde(default)]
    pub trial: Option<serde_json::Value>,
    /// Link to staking discount.
    #[serde(default)]
    pub staking_link: Option<serde_json::Value>,
    /// Active staking discount.
    #[serde(default)]
    pub active_staking_discount: Option<serde_json::Value>,
    /// Fee trial escrow.
    #[serde(default)]
    pub fee_trial_escrow: Option<String>,
    /// Next trial available timestamp.
    #[serde(default)]
    pub next_trial_available_timestamp: Option<u64>,
}

/// User rate limit information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRateLimit {
    pub cum_vlm: Decimal,
    pub n_requests_used: u64,
    pub n_requests_cap: u64,
    #[serde(default)]
    pub n_requests_surplus: Option<u64>,
}

/// Perp asset context (funding rate, mark price, open interest, etc).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpAssetCtx {
    pub day_ntl_vlm: Decimal,
    pub funding: Decimal,
    #[serde(default)]
    pub impact_pxs: Option<Vec<String>>,
    pub mark_px: Decimal,
    pub mid_px: Option<Decimal>,
    pub open_interest: Decimal,
    pub oracle_px: Decimal,
    pub premium: Option<Decimal>,
    pub prev_day_px: Decimal,
    #[serde(default)]
    pub day_base_vlm: Option<Decimal>,
}

/// Spot asset context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotAssetCtx {
    pub day_ntl_vlm: Decimal,
    pub mark_px: Decimal,
    pub mid_px: Option<Decimal>,
    pub prev_day_px: Decimal,
}

/// User funding delta.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFundingDelta {
    #[serde(rename = "type")]
    pub delta_type: String,
    pub coin: String,
    pub usdc: Decimal,
    pub szi: Decimal,
    pub funding_rate: Decimal,
    #[serde(default)]
    pub n_samples: Option<u64>,
}

/// User funding entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFundingEntry {
    pub delta: UserFundingDelta,
    pub hash: String,
    pub time: u64,
}

/// Predicted funding for a venue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictedFundingVenue {
    pub funding_rate: Decimal,
    pub next_funding_time: u64,
}

/// Staking delegation entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delegation {
    pub validator: Address,
    pub amount: Decimal,
    pub locked_until_timestamp: Option<u64>,
}

/// Delegation summary.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegatorSummary {
    pub delegated: Decimal,
    pub undelegated: Decimal,
    pub total_pending_withdrawal: Decimal,
    pub n_pending_withdrawals: u64,
}

/// Perp deploy auction status.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployAuctionStatus {
    pub start_time_seconds: u64,
    pub duration_seconds: u64,
    pub start_gas: Decimal,
    pub current_gas: Decimal,
    pub end_gas: Option<Decimal>,
}

/// HIP-3 DEX limits.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpDexLimits {
    pub total_oi_cap: Option<Decimal>,
    pub oi_sz_cap_per_perp: Option<Decimal>,
    pub max_transfer_ntl: Option<Decimal>,
    #[serde(default)]
    pub coin_to_oi_cap: Option<serde_json::Value>,
}

/// HIP-3 DEX status.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpDexStatus {
    pub total_net_deposit: Decimal,
}

/// Token details from `tokenDetails` info request.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenDetails {
    pub name: String,
    #[serde(default)]
    pub max_supply: Option<Decimal>,
    pub total_supply: Option<Decimal>,
    pub circulating_supply: Option<Decimal>,
    pub sz_decimals: i64,
    pub wei_decimals: i64,
    #[serde(default)]
    pub mid_px: Option<Decimal>,
    #[serde(default)]
    pub mark_px: Option<Decimal>,
    #[serde(default)]
    pub prev_day_px: Option<Decimal>,
    #[serde(default)]
    pub genesis: Option<serde_json::Value>,
    #[serde(default)]
    pub deployer: Option<Address>,
    #[serde(default)]
    pub deploy_gas: Option<u64>,
    #[serde(default)]
    pub deploy_time: Option<u64>,
    #[serde(default)]
    pub seeded_usdc: Option<Decimal>,
    #[serde(default)]
    pub future_emissions: Option<serde_json::Value>,
    #[serde(default)]
    pub non_circulating_user_balances: Option<serde_json::Value>,
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
#[derive(Debug, Clone, Serialize, Deserialize, derive_more::Display)]
#[display("{}:{}", _0.name, _0.token_id)]
pub struct SendToken(pub SpotToken);

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

/// Raw gossip priority auction slot data returned by the Hyperliquid API.
///
/// Each element of the outer `slots` array corresponds to one Dutch auction slot
/// (indices 0–4). Lower index = higher priority (~10 ms faster per slot level).
///
/// ## Price discovery
///
/// The current price decreases linearly over the `duration_seconds` window starting at
/// `start_time_seconds`. Callers can compute the live price with:
///
/// ```ignore
/// let now = chrono::Utc::now().timestamp() as u64;
/// let elapsed = now.saturating_sub(slot.start_time_seconds);
/// let progress = elapsed as f64 / slot.duration_seconds as f64; // 0.0 → 1.0
/// let start: Decimal = slot.start_gas.parse()?;
/// let end: Decimal = slot.end_gas.as_ref().and_then(|s| s.parse().ok()).unwrap_or(start);
/// let current_price = start - (start - end) * Decimal::from_f64_retain(progress).unwrap();
/// ```
///
/// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/priority-fees>
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GossipPrioritySlot {
    /// Unix timestamp (seconds) when this auction cycle started.
    pub start_time_seconds: u64,
    /// Duration of each Dutch auction cycle in seconds (typically 180).
    pub duration_seconds: u64,
    pub start_gas: Decimal,
    #[serde(default)]
    pub current_gas: Option<Decimal>,
    #[serde(default)]
    pub end_gas: Option<Decimal>,
}

/// Gossip priority auction status returned by the `/info` endpoint.
///
/// ## Response shape
///
/// The raw JSON is a 2-element array:
/// ```json
/// [[prev_winner_addrs], [slot0, slot1, slot2, slot3, slot4]]
/// ```
///
/// The first inner array contains the **previous cycle's** winning signer addresses
/// (or `null`) for slots 0–4. The second inner array contains the current Dutch
/// auction parameters for each slot.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(from = "RawGossipPriorityAuctionStatus")]
pub struct GossipPriorityAuctionStatus {
    /// Previous-cycle winners' signer addresses (index = slot id), or `None` if
    /// there was no winner for that slot last cycle.
    #[allow(dead_code)]
    pub prev_winners: Vec<Option<String>>,
    /// Current Dutch auction parameters for all 5 slots (slot id = array index).
    pub slots: Vec<GossipPrioritySlot>,
}

impl std::ops::Deref for GossipPriorityAuctionStatus {
    type Target = Vec<GossipPrioritySlot>;

    fn deref(&self) -> &Self::Target {
        &self.slots
    }
}

// Deserializes [[winners], [slots]] → GossipPriorityAuctionStatus.
#[derive(Deserialize)]
struct RawGossipPriorityAuctionStatus(
    #[allow(dead_code)] Vec<Option<String>>,
    Vec<GossipPrioritySlot>,
);

impl From<RawGossipPriorityAuctionStatus> for GossipPriorityAuctionStatus {
    fn from(raw: RawGossipPriorityAuctionStatus) -> Self {
        Self {
            prev_winners: raw.0,
            slots: raw.1,
        }
    }
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
    pub account_value_history: Vec<(u64, Decimal)>,
    /// Historical PnL values as (timestamp_ms, value) pairs
    pub pnl_history: Vec<(u64, Decimal)>,
    /// Volume for the period
    pub vlm: Decimal,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Serialize)]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    HistoricalOrders {
        user: Address,
    },
    UserFills {
        user: Address,
        #[serde(rename = "aggregateByTime", skip_serializing_if = "Option::is_none")]
        aggregate_by_time: Option<bool>,
    },
    UserFillsByTime {
        user: Address,
        #[serde(rename = "startTime")]
        start_time: u64,
        #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
        end_time: Option<u64>,
        #[serde(rename = "aggregateByTime", skip_serializing_if = "Option::is_none")]
        aggregate_by_time: Option<bool>,
    },
    OrderStatus {
        user: Address,
        #[serde(with = "super::utils::oid_or_cloid")]
        oid: OidOrCloid,
    },
    SpotClearinghouseState {
        user: Address,
    },
    ClearinghouseState {
        user: Address,
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
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
    UserFees {
        user: Address,
    },
    OutcomeMeta,
    /// Query gossip priority auction status.
    GossipPriorityAuctionStatus,
    /// Query account abstraction mode for a user.
    UserAbstraction {
        user: Address,
    },
    /// Check builder fee approval for a user.
    MaxBuilderFee {
        user: Address,
        builder: Address,
    },
    /// Combined perpetual metadata and asset contexts.
    MetaAndAssetCtxs {
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    /// Combined spot metadata and asset contexts.
    SpotMetaAndAssetCtxs,
    /// User's rate limit usage.
    UserRateLimit {
        user: Address,
    },
    /// User's funding history.
    UserFunding {
        user: Address,
        #[serde(rename = "startTime")]
        start_time: u64,
        #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
        end_time: Option<u64>,
    },
    /// User's non-funding ledger updates.
    UserNonFundingLedgerUpdates {
        user: Address,
        #[serde(rename = "startTime")]
        start_time: u64,
        #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
        end_time: Option<u64>,
    },
    /// Predicted funding rates for all coins.
    PredictedFundings,
    /// Coins at open interest cap.
    PerpsAtOpenInterestCap {
        #[serde(skip_serializing_if = "Option::is_none")]
        dex: Option<String>,
    },
    /// Perp deploy auction status.
    PerpDeployAuctionStatus,
    /// User leverage and trade-size limits for a specific asset (info endpoint).
    ActiveAssetData {
        user: Address,
        coin: String,
    },
    /// OI caps and transfer limits for a HIP-3 DEX.
    PerpDexLimits {
        dex: String,
    },
    /// Total net deposit for a HIP-3 DEX.
    PerpDexStatus {
        dex: String,
    },
    /// All DEXs' meta + asset contexts.
    AllPerpMetas,
    /// Category and description for a coin.
    PerpAnnotation {
        coin: String,
    },
    /// All coin categories.
    PerpCategories,
    /// Concise coin annotations.
    PerpConciseAnnotations,
    /// Spot token deploy state for a user.
    SpotDeployState {
        user: Address,
    },
    /// Spot pair deploy auction status.
    SpotPairDeployAuctionStatus,
    /// Detailed token info by tokenId.
    TokenDetails {
        #[serde(rename = "tokenId")]
        token_id: String,
    },
    /// Settled outcome market result.
    SettledOutcome {
        outcome: u64,
    },
    /// User portfolio performance.
    Portfolio {
        user: Address,
    },
    /// Referral state and rewards.
    Referral {
        user: Address,
    },
    /// List of approved builder addresses.
    ApprovedBuilders {
        user: Address,
    },
    /// User's staking delegations.
    Delegations {
        user: Address,
    },
    /// Delegation summary.
    DelegatorSummary {
        user: Address,
    },
    /// Delegation history.
    DelegatorHistory {
        user: Address,
    },
    /// Delegation rewards.
    DelegatorRewards {
        user: Address,
    },
    /// Borrow/lend user state.
    BorrowLendUserState {
        user: Address,
    },
    /// Reserve state for a specific token.
    BorrowLendReserveState {
        token: u32,
    },
    /// All borrow/lend reserve states.
    AllBorrowLendReserveStates,
    /// Aligned quote token info.
    AlignedQuoteTokenInfo {
        token: u32,
    },
    /// TWAP slice fills via info endpoint.
    UserTwapSliceFills {
        user: Address,
    },
    /// L2 order book snapshot.
    L2Book {
        coin: String,
        #[serde(rename = "nSigFigs", skip_serializing_if = "Option::is_none")]
        n_sig_figs: Option<u8>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mantissa: Option<u8>,
    },
    /// Simple open orders (non-frontend).
    OpenOrders {
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
    fn test_fast_asset_ctxs_subscription() {
        let sub = Subscription::FastAssetCtxs;

        let json = serde_json::to_value(&sub).unwrap();
        assert_eq!(json, serde_json::json!({ "type": "fastAssetCtxs" }));
        let deserialized: Subscription = serde_json::from_value(json).unwrap();
        assert_eq!(sub, deserialized);
    }

    #[test]
    fn test_l2_book_fast_subscription() {
        let slow = Subscription::L2Book {
            coin: "BTC".to_string(),
            n_sig_figs: None,
            mantissa: None,
            fast: false,
        };
        assert_eq!(
            serde_json::to_value(&slow).unwrap(),
            serde_json::json!({ "type": "l2Book", "coin": "BTC" })
        );

        let fast = Subscription::L2Book {
            coin: "BTC".to_string(),
            n_sig_figs: None,
            mantissa: None,
            fast: true,
        };
        let json = serde_json::to_value(&fast).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "l2Book", "coin": "BTC", "fast": true })
        );
        let deserialized: Subscription = serde_json::from_value(json).unwrap();
        assert_eq!(fast, deserialized);
    }

    #[test]
    fn test_user_stream_subscription_roundtrip() {
        let user: Address = "0x1234567890abcdef1234567890abcdef12345678"
            .parse()
            .unwrap();
        let subs = vec![
            Subscription::UserEvents { user },
            Subscription::UserTwapSliceFills { user },
            Subscription::UserTwapHistory { user },
            Subscription::ActiveAssetData {
                user,
                coin: "BTC".to_string(),
            },
            Subscription::WebData2 { user, dex: None },
        ];

        for sub in subs {
            let json = serde_json::to_string(&sub).unwrap();
            let parsed: Subscription = serde_json::from_str(&json).unwrap();
            assert_eq!(sub, parsed);
        }
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
            _ => assert!(false, "Expected Incoming::Candle"),
        }
    }

    #[test]
    fn test_incoming_fast_asset_ctxs_decodes_payload() {
        let json = r#"{
            "channel":"fastAssetCtxs",
            "data":"q1ZyCnFWsqpWyk0syg6oULJSsjQ3NTDQM1Wq1VFyDfFAkTI2MzXQMwJLVVRWWfmFuTiiyBuamOoZKdXWAgA="
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::FastAssetCtxs(ctxs) => {
                assert_eq!(ctxs.len(), 3);
                assert_eq!(ctxs["BTC"].mark_px.unwrap().to_string(), "97500.5");
                assert_eq!(ctxs["ETH"].mark_px.unwrap().to_string(), "3650.25");
                assert_eq!(ctxs["xyz:NVDA"].mark_px.unwrap().to_string(), "145.2");
                assert_eq!(ctxs["BTC"].mid_px, None);
            }
            _ => assert!(false, "Expected Incoming::FastAssetCtxs"),
        }
    }

    #[test]
    fn test_fast_asset_ctx_accepts_numbers_and_nulls() {
        let ctx: FastAssetCtx = serde_json::from_value(serde_json::json!({
            "markPx": 97500.5,
            "midPx": null
        }))
        .unwrap();

        assert_eq!(ctx.mark_px.unwrap().to_string(), "97500.5");
        assert_eq!(ctx.mid_px, None);
    }

    #[test]
    fn test_incoming_user_events_funding() {
        let json = r#"{
            "channel":"userEvents",
            "data":{
                "funding":{
                    "time":1710000000123,
                    "coin":"BTC",
                    "usdc":"-1.25",
                    "szi":"0.5",
                    "fundingRate":"0.0001"
                }
            }
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::UserEvents(UserEvent::Funding { funding }) => {
                assert_eq!(funding.coin, "BTC");
                assert_eq!(funding.usdc.to_string(), "-1.25");
                assert_eq!(funding.szi.to_string(), "0.5");
                assert_eq!(funding.funding_rate.to_string(), "0.0001");
            }
            _ => assert!(false, "Expected Incoming::UserEvents::Funding"),
        }
    }

    #[test]
    fn test_incoming_user_events_non_user_cancel() {
        let json = r#"{
            "channel":"userEvents",
            "data":{
                "nonUserCancel":[
                    {"coin":"BTC","oid":77738308}
                ]
            }
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::UserEvents(UserEvent::NonUserCancel { non_user_cancel }) => {
                assert_eq!(non_user_cancel.len(), 1);
                assert_eq!(non_user_cancel[0].coin, "BTC");
                assert_eq!(non_user_cancel[0].oid, 77738308);
            }
            _ => assert!(false, "Expected Incoming::UserEvents::NonUserCancel"),
        }
    }

    #[test]
    fn test_incoming_user_events_unknown_fallback() {
        let json = r#"{
            "channel":"userEvents",
            "data":{"mystery":{"field":1}}
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::UserEvents(UserEvent::Unknown(raw)) => {
                assert_eq!(raw["mystery"]["field"], 1);
            }
            _ => assert!(false, "Expected Incoming::UserEvents::Unknown"),
        }
    }

    #[test]
    fn test_incoming_active_asset_data_mixed_number_formats() {
        let json = r#"{
            "channel":"activeAssetData",
            "data":{
                "user":"0x1234567890abcdef1234567890abcdef12345678",
                "coin":"BTC",
                "leverage":{"type":"cross","value":5},
                "maxTradeSzs":["12.5",8.75],
                "availableToTrade":[3,"4.5"]
            }
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::ActiveAssetData(data) => {
                assert_eq!(data.coin, "BTC");
                assert_eq!(data.leverage.leverage_type, "cross");
                assert_eq!(data.leverage.value.to_string(), "5");
                assert_eq!(
                    data.max_trade_szs_pair(),
                    Some((Decimal::new(125, 1), Decimal::new(875, 2)))
                );
                assert_eq!(
                    data.available_to_trade_pair(),
                    Some((Decimal::new(3, 0), Decimal::new(45, 1)))
                );
            }
            _ => assert!(false, "Expected Incoming::ActiveAssetData"),
        }
    }

    #[test]
    fn test_incoming_user_twap_slice_fills() {
        let json = r#"{
            "channel":"userTwapSliceFills",
            "data":{
                "isSnapshot":true,
                "user":"0x1234567890abcdef1234567890abcdef12345678",
                "twapSliceFills":[
                    {
                        "twapId":42,
                        "fill":{
                            "coin":"BTC",
                            "px":"95000.0",
                            "sz":"0.01",
                            "side":"B",
                            "time":1710000000222,
                            "startPosition":"0.0",
                            "dir":"Open Long",
                            "closedPnl":"0.0",
                            "hash":"0xabc",
                            "oid":1001,
                            "crossed":true,
                            "fee":"-0.01",
                            "tid":555,
                            "feeToken":"USDC"
                        }
                    }
                ]
            }
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::UserTwapSliceFills(payload) => {
                assert!(payload.is_snapshot);
                assert_eq!(payload.twap_slice_fills.len(), 1);
                assert_eq!(payload.twap_slice_fills[0].twap_id, 42);
                assert_eq!(payload.twap_slice_fills[0].fill.coin, "BTC");
                assert_eq!(payload.twap_slice_fills[0].fill.px.to_string(), "95000.0");
                assert_eq!(
                    payload.twap_slice_fills[0].fill.dir,
                    FillDirection::OpenLong
                );
            }
            _ => assert!(false, "Expected Incoming::UserTwapSliceFills"),
        }
    }

    #[test]
    fn fill_direction_serde_values() {
        let cases = [
            (FillDirection::OpenLong, "Open Long"),
            (FillDirection::OpenShort, "Open Short"),
            (FillDirection::CloseLong, "Close Long"),
            (FillDirection::CloseShort, "Close Short"),
            (FillDirection::LongToShort, "Long > Short"),
            (FillDirection::ShortToLong, "Short > Long"),
            (FillDirection::LiquidatedCrossLong, "Liquidated Cross Long"),
            (
                FillDirection::LiquidatedCrossShort,
                "Liquidated Cross Short",
            ),
            (
                FillDirection::LiquidatedIsolatedLong,
                "Liquidated Isolated Long",
            ),
            (
                FillDirection::LiquidatedIsolatedShort,
                "Liquidated Isolated Short",
            ),
            (FillDirection::AutoDeleveraging, "Auto-Deleveraging"),
            (
                FillDirection::PartialBorrowLiquidation,
                "Partial Borrow Liquidation",
            ),
            (
                FillDirection::BackstopBorrowLiquidation,
                "Backstop Borrow Liquidation",
            ),
            (FillDirection::Settlement, "Settlement"),
            (FillDirection::NetChildVaults, "Net Child Vaults"),
            (FillDirection::Buy, "Buy"),
            (FillDirection::Sell, "Sell"),
            (FillDirection::SpotDustConversion, "Spot Dust Conversion"),
        ];

        for (direction, wire) in cases {
            assert_eq!(direction.as_str(), wire);
            assert_eq!(direction.to_string(), wire);
            assert_eq!(
                serde_json::to_string(&direction).unwrap(),
                format!("{wire:?}")
            );
            assert_eq!(
                serde_json::from_str::<FillDirection>(&format!("{wire:?}")).unwrap(),
                direction
            );
        }
    }

    #[test]
    fn test_incoming_user_twap_history() {
        let json = r#"{
            "channel":"userTwapHistory",
            "data":{
                "isSnapshot":false,
                "user":"0x1234567890abcdef1234567890abcdef12345678",
                "history":[
                    {
                        "state":{
                            "coin":"BTC",
                            "user":"0x1234567890abcdef1234567890abcdef12345678",
                            "side":"buy",
                            "sz":"0.5",
                            "executedSz":0.25,
                            "executedNtl":"23750.0",
                            "minutes":30,
                            "reduceOnly":false,
                            "randomize":true,
                            "timestamp":1710000000333
                        },
                        "status":{
                            "status":"finished",
                            "description":"completed"
                        },
                        "time":1710001800333
                    }
                ]
            }
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::UserTwapHistory(payload) => {
                assert!(!payload.is_snapshot);
                assert_eq!(payload.history.len(), 1);
                let item = &payload.history[0];
                assert_eq!(item.state.coin, "BTC");
                assert_eq!(item.state.sz.to_string(), "0.5");
                assert_eq!(item.state.executed_sz.to_string(), "0.25");
                assert_eq!(item.status.description.as_deref(), Some("completed"));
                assert!(matches!(item.status.status, TwapStatus::Finished));
            }
            _ => assert!(false, "Expected Incoming::UserTwapHistory"),
        }
    }

    #[test]
    fn test_incoming_user_twap_history_without_description() {
        let json = r#"{
            "channel":"userTwapHistory",
            "data":{
                "isSnapshot":true,
                "user":"0x1234567890abcdef1234567890abcdef12345678",
                "history":[
                    {
                        "state":{
                            "coin":"BTC",
                            "user":"0x1234567890abcdef1234567890abcdef12345678",
                            "side":"buy",
                            "sz":"0.5",
                            "executedSz":"0.0",
                            "executedNtl":"0.0",
                            "minutes":30,
                            "reduceOnly":false,
                            "randomize":false,
                            "timestamp":1710000000333
                        },
                        "status":{
                            "status":"activated"
                        },
                        "time":1710000000333
                    }
                ]
            }
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::UserTwapHistory(payload) => {
                assert!(payload.is_snapshot);
                assert_eq!(payload.history.len(), 1);
                let item = &payload.history[0];
                assert!(matches!(item.status.status, TwapStatus::Activated));
                assert_eq!(item.status.description, None);
            }
            _ => assert!(false, "Expected Incoming::UserTwapHistory"),
        }
    }

    #[test]
    fn test_incoming_web_data2_raw_payload() {
        let json = r#"{
            "channel":"webData2",
            "data":{
                "clearinghouseState":{"time":1710002000000},
                "openOrders":[{"oid":1234}]
            }
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::WebData2 { data: payload, .. } => {
                assert_eq!(payload["clearinghouseState"]["time"], 1710002000000u64);
                assert_eq!(payload["openOrders"][0]["oid"], 1234u64);
            }
            _ => assert!(false, "Expected Incoming::WebData2"),
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

    #[test]
    fn trade_deserializes_with_users() {
        let json = r#"{
            "coin": "BTC",
            "side": "B",
            "px": "97000.0",
            "sz": "0.5",
            "time": 1700000000000,
            "hash": "0xabc123",
            "tid": 42,
            "users": ["0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222"]
        }"#;
        let trade: Trade = serde_json::from_str(json).unwrap();
        let buyer: Address = "0x1111111111111111111111111111111111111111"
            .parse()
            .unwrap();
        let seller: Address = "0x2222222222222222222222222222222222222222"
            .parse()
            .unwrap();
        assert_eq!(trade.users[0], buyer);
        assert_eq!(trade.users[1], seller);
    }

    #[test]
    fn trade_deserializes_without_users() {
        let json = r#"{
            "coin": "BTC",
            "side": "A",
            "px": "97000.0",
            "sz": "0.5",
            "time": 1700000000000,
            "hash": "0xabc123",
            "tid": 42
        }"#;
        let trade: Trade = serde_json::from_str(json).unwrap();
        assert_eq!(trade.users, [Address::ZERO, Address::ZERO]);
    }

    #[test]
    fn taker_address_returns_buyer_on_bid() {
        let json = r#"{
            "coin": "BTC",
            "side": "B",
            "px": "97000.0",
            "sz": "0.5",
            "time": 1700000000000,
            "hash": "0xabc123",
            "tid": 42,
            "users": ["0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222"]
        }"#;
        let trade: Trade = serde_json::from_str(json).unwrap();
        let buyer: Address = "0x1111111111111111111111111111111111111111"
            .parse()
            .unwrap();
        assert_eq!(trade.taker_address(), buyer);
    }

    #[test]
    fn taker_address_returns_seller_on_ask() {
        let json = r#"{
            "coin": "BTC",
            "side": "A",
            "px": "97000.0",
            "sz": "0.5",
            "time": 1700000000000,
            "hash": "0xabc123",
            "tid": 42,
            "users": ["0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222"]
        }"#;
        let trade: Trade = serde_json::from_str(json).unwrap();
        let seller: Address = "0x2222222222222222222222222222222222222222"
            .parse()
            .unwrap();
        assert_eq!(trade.taker_address(), seller);
    }

    #[test]
    fn taker_address_returns_zero_when_users_absent() {
        let json = r#"{
            "coin": "BTC",
            "side": "B",
            "px": "97000.0",
            "sz": "0.5",
            "time": 1700000000000,
            "hash": "0xabc123",
            "tid": 42
        }"#;
        let trade: Trade = serde_json::from_str(json).unwrap();
        assert_eq!(trade.taker_address(), Address::ZERO);
    }

    #[test]
    fn maker_address_returns_seller_on_bid() {
        let json = r#"{
            "coin": "BTC",
            "side": "B",
            "px": "97000.0",
            "sz": "0.5",
            "time": 1700000000000,
            "hash": "0xabc123",
            "tid": 42,
            "users": ["0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222"]
        }"#;
        let trade: Trade = serde_json::from_str(json).unwrap();
        let seller: Address = "0x2222222222222222222222222222222222222222"
            .parse()
            .unwrap();
        assert_eq!(trade.maker_address(), seller);
    }

    #[test]
    fn maker_address_returns_buyer_on_ask() {
        let json = r#"{
            "coin": "BTC",
            "side": "A",
            "px": "97000.0",
            "sz": "0.5",
            "time": 1700000000000,
            "hash": "0xabc123",
            "tid": 42,
            "users": ["0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222"]
        }"#;
        let trade: Trade = serde_json::from_str(json).unwrap();
        let buyer: Address = "0x1111111111111111111111111111111111111111"
            .parse()
            .unwrap();
        assert_eq!(trade.maker_address(), buyer);
    }

    // ─── OrderGrouping (write priority) ───────────────────────────────────────

    #[test]
    fn order_grouping_na_serialize() {
        assert_eq!(
            serde_json::to_string(&OrderGrouping::Na).unwrap(),
            r#""na""#
        );
    }

    #[test]
    fn order_grouping_priority_rate_serialize() {
        let json = serde_json::to_string(&OrderGrouping::PriorityRate(80_000)).unwrap();
        assert_eq!(json, r#"{"p":80000}"#);
    }

    #[test]
    fn order_grouping_deserialize_all_variants() {
        assert!(matches!(
            serde_json::from_str::<OrderGrouping>(r#""na""#).unwrap(),
            OrderGrouping::Na
        ));
        assert!(matches!(
            serde_json::from_str::<OrderGrouping>(r#""normalTpsl""#).unwrap(),
            OrderGrouping::NormalTpsl
        ));
        assert!(matches!(
            serde_json::from_str::<OrderGrouping>(r#"{"p":80000}"#).unwrap(),
            OrderGrouping::PriorityRate(80_000)
        ));
    }

    #[test]
    fn batch_order_with_priority_rate_roundtrip() {
        use rust_decimal::dec;

        let batch = BatchOrder {
            orders: vec![OrderRequest {
                asset: 0,
                is_buy: true,
                limit_px: dec!(50000),
                sz: dec!(0.1),
                reduce_only: false,
                order_type: OrderTypePlacement::Limit {
                    tif: TimeInForce::Ioc,
                },
                cloid: Default::default(),
            }],
            grouping: OrderGrouping::PriorityRate(80_000),
            builder: None,
        };

        let json = serde_json::to_string(&batch).unwrap();
        let parsed: BatchOrder = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed.grouping,
            OrderGrouping::PriorityRate(80_000)
        ));
    }

    #[test]
    fn test_incoming_user_channel_fills() {
        // Hyperliquid sends fill notifications on channel "user" (not "userEvents").
        // The payload matches UserEvent::Fills — just a {"fills":[...]} object.
        // This reproduces the real wire-format messages from production:
        //   {"channel":"user","data":{"fills":[{"coin":"BTC",...}]}}
        let json = r#"{
            "channel": "user",
            "data": {
                "fills": [
                    {
                        "coin": "ETH",
                        "px": "3500.50",
                        "sz": "0.5",
                        "side": "A",
                        "time": 1700000000000,
                        "startPosition": "1.0",
                        "dir": "Close Short",
                        "closedPnl": "125.50",
                        "hash": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                        "oid": 1234567890,
                        "crossed": false,
                        "fee": "0.125",
                        "tid": 9876543210,
                        "feeToken": "USDC",
                        "twapId": null
                    }
                ]
            }
        }"#;

        let incoming: Incoming = serde_json::from_str(json).unwrap();
        match incoming {
            Incoming::UserEvents(UserEvent::Fills { fills }) => {
                assert_eq!(fills.len(), 1);
                assert_eq!(fills[0].coin, "ETH");
                assert_eq!(fills[0].px.to_string(), "3500.50");
                assert_eq!(fills[0].sz.to_string(), "0.5");
            }
            _ => {
                assert!(
                    false,
                    "Expected Incoming::UserEvents(UserEvent::Fills {{ .. }}), got {incoming:?}"
                )
            }
        }
    }

    mod info_request_serialization {
        use super::*;
        use alloy::primitives::address;
        use either::Either;

        const USER: Address = address!("0x0000000000000000000000000000000000001234");
        const BUILDER: Address = address!("0x0000000000000000000000000000000000005678");

        fn assert_json(req: InfoRequest, expected: serde_json::Value) {
            let serialized = serde_json::to_value(&req).unwrap();
            assert_eq!(serialized, expected, "InfoRequest::{req:?}");
        }

        #[test]
        fn meta() {
            assert_json(
                InfoRequest::Meta { dex: None },
                serde_json::json!({"type": "meta"}),
            );
            assert_json(
                InfoRequest::Meta {
                    dex: Some("HyperBTC".into()),
                },
                serde_json::json!({"type": "meta", "dex": "HyperBTC"}),
            );
        }

        #[test]
        fn spot_meta() {
            assert_json(
                InfoRequest::SpotMeta,
                serde_json::json!({"type": "spotMeta"}),
            );
        }

        #[test]
        fn perp_dexs() {
            assert_json(
                InfoRequest::PerpDexs,
                serde_json::json!({"type": "perpDexs"}),
            );
        }

        #[test]
        fn frontend_open_orders() {
            assert_json(
                InfoRequest::FrontendOpenOrders {
                    user: USER,
                    dex: None,
                },
                serde_json::json!({"type": "frontendOpenOrders", "user": "0x0000000000000000000000000000000000001234"}),
            );
            assert_json(
                InfoRequest::FrontendOpenOrders {
                    user: USER,
                    dex: Some("HyperBTC".into()),
                },
                serde_json::json!({"type": "frontendOpenOrders", "user": "0x0000000000000000000000000000000000001234", "dex": "HyperBTC"}),
            );
        }

        #[test]
        fn historical_orders() {
            assert_json(
                InfoRequest::HistoricalOrders { user: USER },
                serde_json::json!({"type": "historicalOrders", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn user_fills() {
            assert_json(
                InfoRequest::UserFills {
                    user: USER,
                    aggregate_by_time: None,
                },
                serde_json::json!({"type": "userFills", "user": "0x0000000000000000000000000000000000001234"}),
            );
            assert_json(
                InfoRequest::UserFills {
                    user: USER,
                    aggregate_by_time: Some(true),
                },
                serde_json::json!({"type": "userFills", "user": "0x0000000000000000000000000000000000001234", "aggregateByTime": true}),
            );
        }

        #[test]
        fn user_fills_by_time() {
            assert_json(
                InfoRequest::UserFillsByTime {
                    user: USER,
                    start_time: 1000,
                    end_time: None,
                    aggregate_by_time: None,
                },
                serde_json::json!({"type": "userFillsByTime", "user": "0x0000000000000000000000000000000000001234", "startTime": 1000}),
            );
            assert_json(
                InfoRequest::UserFillsByTime {
                    user: USER,
                    start_time: 1000,
                    end_time: Some(2000),
                    aggregate_by_time: Some(true),
                },
                serde_json::json!({"type": "userFillsByTime", "user": "0x0000000000000000000000000000000000001234", "startTime": 1000, "endTime": 2000, "aggregateByTime": true}),
            );
        }

        #[test]
        fn order_status() {
            assert_json(
                InfoRequest::OrderStatus {
                    user: USER,
                    oid: Either::Left(42),
                },
                serde_json::json!({"type": "orderStatus", "user": "0x0000000000000000000000000000000000001234", "oid": 42}),
            );
        }

        #[test]
        fn spot_clearinghouse_state() {
            assert_json(
                InfoRequest::SpotClearinghouseState { user: USER },
                serde_json::json!({"type": "spotClearinghouseState", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn clearinghouse_state() {
            assert_json(
                InfoRequest::ClearinghouseState {
                    user: USER,
                    dex: None,
                },
                serde_json::json!({"type": "clearinghouseState", "user": "0x0000000000000000000000000000000000001234"}),
            );
            assert_json(
                InfoRequest::ClearinghouseState {
                    user: USER,
                    dex: Some("HyperBTC".into()),
                },
                serde_json::json!({"type": "clearinghouseState", "user": "0x0000000000000000000000000000000000001234", "dex": "HyperBTC"}),
            );
        }

        #[test]
        fn all_mids() {
            assert_json(
                InfoRequest::AllMids { dex: None },
                serde_json::json!({"type": "allMids"}),
            );
            assert_json(
                InfoRequest::AllMids {
                    dex: Some("HyperBTC".into()),
                },
                serde_json::json!({"type": "allMids", "dex": "HyperBTC"}),
            );
        }

        #[test]
        fn candle_snapshot() {
            assert_json(
                InfoRequest::CandleSnapshot {
                    req: CandleSnapshotRequest {
                        coin: "BTC".into(),
                        interval: CandleInterval::FifteenMinutes,
                        start_time: 1000,
                        end_time: 2000,
                    },
                },
                serde_json::json!({"type": "candleSnapshot", "req": {"coin": "BTC", "interval": "15m", "startTime": 1000, "endTime": 2000}}),
            );
        }

        #[test]
        fn user_to_multi_sig_signers() {
            assert_json(
                InfoRequest::UserToMultiSigSigners { user: USER },
                serde_json::json!({"type": "userToMultiSigSigners", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn extra_agents() {
            assert_json(
                InfoRequest::ExtraAgents { user: USER },
                serde_json::json!({"type": "extraAgents", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn funding_history() {
            assert_json(
                InfoRequest::FundingHistory {
                    coin: "BTC".into(),
                    start_time: 1000,
                    end_time: None,
                },
                serde_json::json!({"type": "fundingHistory", "coin": "BTC", "startTime": 1000}),
            );
            assert_json(
                InfoRequest::FundingHistory {
                    coin: "ETH".into(),
                    start_time: 1000,
                    end_time: Some(2000),
                },
                serde_json::json!({"type": "fundingHistory", "coin": "ETH", "startTime": 1000, "endTime": 2000}),
            );
        }

        #[test]
        fn vault_details() {
            assert_json(
                InfoRequest::VaultDetails {
                    vault_address: USER,
                    user: None,
                },
                serde_json::json!({"type": "vaultDetails", "vaultAddress": "0x0000000000000000000000000000000000001234"}),
            );
            assert_json(
                InfoRequest::VaultDetails {
                    vault_address: USER,
                    user: Some(BUILDER),
                },
                serde_json::json!({"type": "vaultDetails", "vaultAddress": "0x0000000000000000000000000000000000001234", "user": "0x0000000000000000000000000000000000005678"}),
            );
        }

        #[test]
        fn user_vault_equities() {
            assert_json(
                InfoRequest::UserVaultEquities { user: USER },
                serde_json::json!({"type": "userVaultEquities", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn user_role() {
            assert_json(
                InfoRequest::UserRole { user: USER },
                serde_json::json!({"type": "userRole", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn sub_accounts() {
            assert_json(
                InfoRequest::SubAccounts { user: USER },
                serde_json::json!({"type": "subAccounts", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn user_fees() {
            assert_json(
                InfoRequest::UserFees { user: USER },
                serde_json::json!({"type": "userFees", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn outcome_meta() {
            assert_json(
                InfoRequest::OutcomeMeta,
                serde_json::json!({"type": "outcomeMeta"}),
            );
        }

        #[test]
        fn gossip_priority_auction_status() {
            assert_json(
                InfoRequest::GossipPriorityAuctionStatus,
                serde_json::json!({"type": "gossipPriorityAuctionStatus"}),
            );
        }

        #[test]
        fn user_abstraction() {
            assert_json(
                InfoRequest::UserAbstraction { user: USER },
                serde_json::json!({"type": "userAbstraction", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn max_builder_fee() {
            assert_json(
                InfoRequest::MaxBuilderFee {
                    user: USER,
                    builder: BUILDER,
                },
                serde_json::json!({"type": "maxBuilderFee", "user": "0x0000000000000000000000000000000000001234", "builder": "0x0000000000000000000000000000000000005678"}),
            );
        }

        #[test]
        fn meta_and_asset_ctxs() {
            assert_json(
                InfoRequest::MetaAndAssetCtxs { dex: None },
                serde_json::json!({"type": "metaAndAssetCtxs"}),
            );
            assert_json(
                InfoRequest::MetaAndAssetCtxs {
                    dex: Some("HyperBTC".into()),
                },
                serde_json::json!({"type": "metaAndAssetCtxs", "dex": "HyperBTC"}),
            );
        }

        #[test]
        fn spot_meta_and_asset_ctxs() {
            assert_json(
                InfoRequest::SpotMetaAndAssetCtxs,
                serde_json::json!({"type": "spotMetaAndAssetCtxs"}),
            );
        }

        #[test]
        fn user_rate_limit() {
            assert_json(
                InfoRequest::UserRateLimit { user: USER },
                serde_json::json!({"type": "userRateLimit", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn user_funding() {
            assert_json(
                InfoRequest::UserFunding {
                    user: USER,
                    start_time: 1000,
                    end_time: None,
                },
                serde_json::json!({"type": "userFunding", "user": "0x0000000000000000000000000000000000001234", "startTime": 1000}),
            );
            assert_json(
                InfoRequest::UserFunding {
                    user: USER,
                    start_time: 1000,
                    end_time: Some(2000),
                },
                serde_json::json!({"type": "userFunding", "user": "0x0000000000000000000000000000000000001234", "startTime": 1000, "endTime": 2000}),
            );
        }

        #[test]
        fn user_non_funding_ledger_updates() {
            assert_json(
                InfoRequest::UserNonFundingLedgerUpdates {
                    user: USER,
                    start_time: 1000,
                    end_time: None,
                },
                serde_json::json!({"type": "userNonFundingLedgerUpdates", "user": "0x0000000000000000000000000000000000001234", "startTime": 1000}),
            );
        }

        #[test]
        fn predicted_fundings() {
            assert_json(
                InfoRequest::PredictedFundings,
                serde_json::json!({"type": "predictedFundings"}),
            );
        }

        #[test]
        fn perps_at_open_interest_cap() {
            assert_json(
                InfoRequest::PerpsAtOpenInterestCap { dex: None },
                serde_json::json!({"type": "perpsAtOpenInterestCap"}),
            );
            assert_json(
                InfoRequest::PerpsAtOpenInterestCap {
                    dex: Some("HyperBTC".into()),
                },
                serde_json::json!({"type": "perpsAtOpenInterestCap", "dex": "HyperBTC"}),
            );
        }

        #[test]
        fn perp_deploy_auction_status() {
            assert_json(
                InfoRequest::PerpDeployAuctionStatus,
                serde_json::json!({"type": "perpDeployAuctionStatus"}),
            );
        }

        #[test]
        fn active_asset_data() {
            assert_json(
                InfoRequest::ActiveAssetData {
                    user: USER,
                    coin: "BTC".into(),
                },
                serde_json::json!({"type": "activeAssetData", "user": "0x0000000000000000000000000000000000001234", "coin": "BTC"}),
            );
        }

        #[test]
        fn perp_dex_limits() {
            assert_json(
                InfoRequest::PerpDexLimits {
                    dex: "HyperBTC".into(),
                },
                serde_json::json!({"type": "perpDexLimits", "dex": "HyperBTC"}),
            );
        }

        #[test]
        fn perp_dex_status() {
            assert_json(
                InfoRequest::PerpDexStatus {
                    dex: "HyperBTC".into(),
                },
                serde_json::json!({"type": "perpDexStatus", "dex": "HyperBTC"}),
            );
        }

        #[test]
        fn all_perp_metas() {
            assert_json(
                InfoRequest::AllPerpMetas,
                serde_json::json!({"type": "allPerpMetas"}),
            );
        }

        #[test]
        fn perp_annotation() {
            assert_json(
                InfoRequest::PerpAnnotation { coin: "BTC".into() },
                serde_json::json!({"type": "perpAnnotation", "coin": "BTC"}),
            );
        }

        #[test]
        fn perp_categories() {
            assert_json(
                InfoRequest::PerpCategories,
                serde_json::json!({"type": "perpCategories"}),
            );
        }

        #[test]
        fn perp_concise_annotations() {
            assert_json(
                InfoRequest::PerpConciseAnnotations,
                serde_json::json!({"type": "perpConciseAnnotations"}),
            );
        }

        #[test]
        fn spot_deploy_state() {
            assert_json(
                InfoRequest::SpotDeployState { user: USER },
                serde_json::json!({"type": "spotDeployState", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn spot_pair_deploy_auction_status() {
            assert_json(
                InfoRequest::SpotPairDeployAuctionStatus,
                serde_json::json!({"type": "spotPairDeployAuctionStatus"}),
            );
        }

        #[test]
        fn token_details() {
            assert_json(
                InfoRequest::TokenDetails {
                    token_id: "0xc4bf3f870c0e9465323c0b6ed28096c2".into(),
                },
                serde_json::json!({"type": "tokenDetails", "tokenId": "0xc4bf3f870c0e9465323c0b6ed28096c2"}),
            );
        }

        #[test]
        fn settled_outcome() {
            assert_json(
                InfoRequest::SettledOutcome { outcome: 1273 },
                serde_json::json!({"type": "settledOutcome", "outcome": 1273}),
            );
        }

        #[test]
        fn portfolio() {
            assert_json(
                InfoRequest::Portfolio { user: USER },
                serde_json::json!({"type": "portfolio", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn referral() {
            assert_json(
                InfoRequest::Referral { user: USER },
                serde_json::json!({"type": "referral", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn approved_builders() {
            assert_json(
                InfoRequest::ApprovedBuilders { user: USER },
                serde_json::json!({"type": "approvedBuilders", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn delegations() {
            assert_json(
                InfoRequest::Delegations { user: USER },
                serde_json::json!({"type": "delegations", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn delegator_summary() {
            assert_json(
                InfoRequest::DelegatorSummary { user: USER },
                serde_json::json!({"type": "delegatorSummary", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn delegator_history() {
            assert_json(
                InfoRequest::DelegatorHistory { user: USER },
                serde_json::json!({"type": "delegatorHistory", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn delegator_rewards() {
            assert_json(
                InfoRequest::DelegatorRewards { user: USER },
                serde_json::json!({"type": "delegatorRewards", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn borrow_lend_user_state() {
            assert_json(
                InfoRequest::BorrowLendUserState { user: USER },
                serde_json::json!({"type": "borrowLendUserState", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn borrow_lend_reserve_state() {
            assert_json(
                InfoRequest::BorrowLendReserveState { token: 1 },
                serde_json::json!({"type": "borrowLendReserveState", "token": 1}),
            );
        }

        #[test]
        fn all_borrow_lend_reserve_states() {
            assert_json(
                InfoRequest::AllBorrowLendReserveStates,
                serde_json::json!({"type": "allBorrowLendReserveStates"}),
            );
        }

        #[test]
        fn aligned_quote_token_info() {
            assert_json(
                InfoRequest::AlignedQuoteTokenInfo { token: 5 },
                serde_json::json!({"type": "alignedQuoteTokenInfo", "token": 5}),
            );
        }

        #[test]
        fn user_twap_slice_fills() {
            assert_json(
                InfoRequest::UserTwapSliceFills { user: USER },
                serde_json::json!({"type": "userTwapSliceFills", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }

        #[test]
        fn l2_book() {
            assert_json(
                InfoRequest::L2Book {
                    coin: "BTC".into(),
                    n_sig_figs: None,
                    mantissa: None,
                },
                serde_json::json!({"type": "l2Book", "coin": "BTC"}),
            );
            assert_json(
                InfoRequest::L2Book {
                    coin: "ETH".into(),
                    n_sig_figs: Some(5),
                    mantissa: Some(2),
                },
                serde_json::json!({"type": "l2Book", "coin": "ETH", "nSigFigs": 5, "mantissa": 2}),
            );
        }

        #[test]
        fn open_orders() {
            assert_json(
                InfoRequest::OpenOrders { user: USER },
                serde_json::json!({"type": "openOrders", "user": "0x0000000000000000000000000000000000001234"}),
            );
        }
    }
}
