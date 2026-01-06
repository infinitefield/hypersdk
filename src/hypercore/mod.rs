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
//! use rust_decimal_macros::dec;
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
//! };
//!
//! let nonce = chrono::Utc::now().timestamp_millis() as u64;
//! let result = client.place(&signer, order, nonce, None, None).await?;
//! # Ok(())
//! # }
//! ```

pub mod http;
pub mod types;
pub mod ws;

use std::{fmt, hash::Hash};

use alloy::primitives::{B128, U256, address};
/// Reimport signers.
pub use alloy::signers::local::PrivateKeySigner;
use anyhow::Context;
use either::Either;
use reqwest::IntoUrl;
use rust_decimal::{Decimal, MathematicalOps, RoundingStrategy, prelude::ToPrimitive};
use serde::Deserialize;
use url::Url;

use crate::{
    Address,
    hypercore::types::Side,
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

/// Chain identifier for Hyperliquid operations.
///
/// This determines which network-specific constants to use for signatures and operations.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    derive_more::Display,
    derive_more::FromStr,
    derive_more::IsVariant,
)]
pub enum Chain {
    /// Mainnet chain
    Mainnet,
    /// Testnet chain
    Testnet,
}

impl Chain {
    /// Returns the multisig signature chain ID for this chain.
    ///
    /// # Example
    ///
    /// ```
    /// use hypersdk::hypercore::Chain;
    ///
    /// assert_eq!(Chain::Mainnet.multisig_chain_id(), "0x66eee");
    /// assert_eq!(Chain::Testnet.multisig_chain_id(), "0x66eef");
    /// ```
    #[must_use]
    pub const fn multisig_chain_id(&self) -> &'static str {
        match self {
            Self::Mainnet => MAINNET_MULTISIG_CHAIN_ID,
            Self::Testnet => TESTNET_MULTISIG_CHAIN_ID,
        }
    }
}

/// Arbitrum mainnet chain ID used for EIP-712 signatures.
///
/// This is required when signing transactions that involve cross-chain operations
/// or transfers between HyperCore and HyperEVM.
pub const ARBITRUM_SIGNATURE_CHAIN_ID: &str = "0xa4b1";

/// Mainnet L1 multisig signature chain ID.
///
/// Use this when executing multisig transactions on mainnet.
pub const MAINNET_MULTISIG_CHAIN_ID: &str = "0x66eee";

/// Testnet L1 multisig signature chain ID.
///
/// Use this when executing multisig transactions on testnet.
pub const TESTNET_MULTISIG_CHAIN_ID: &str = "0x66eef";

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

/// Price tick table for determining valid price increments.
///
/// Hyperliquid enforces different tick size constraints for spot and perpetual markets.
/// This table provides O(1) tick size calculation using a unified significant figures algorithm.
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
pub struct PriceTickTable {
    /// Maximum decimal places allowed for this market.
    /// - Spot: max_decimals = 8 - sz_decimals
    /// - Perp: max_decimals = 6 - sz_decimals
    max_decimals: i64,
}

impl PriceTickTable {
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
    /// ```no_run
    /// # use hypersdk::hypercore::PriceTickTable;
    /// # use rust_decimal_macros::dec;
    /// // BTC perp: max_decimals = 1
    /// let btc_table = PriceTickTable { max_decimals: 1 };
    ///
    /// // 5-digit price: decimals = 5-5 = 0, clamped to 0 → tick = 1
    /// assert_eq!(btc_table.tick_for(dec!(93231)), Some(dec!(1)));
    ///
    /// // SOL perp: max_decimals = 4
    /// let sol_table = PriceTickTable { max_decimals: 4 };
    ///
    /// // 3-digit price: decimals = 5-3 = 2, clamped to 2 → tick = 0.01
    /// assert_eq!(sol_table.tick_for(dec!(137)), Some(dec!(0.01)));
    ///
    /// // 2-digit price: decimals = 5-2 = 3, clamped to 3 → tick = 0.001
    /// assert_eq!(sol_table.tick_for(dec!(99)), Some(dec!(0.001)));
    /// ```
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
    /// ```no_run
    /// # use hypersdk::hypercore::PriceTickTable;
    /// # use rust_decimal_macros::dec;
    /// # let table = PriceTickTable { max_decimals: 0 };
    /// let price = dec!(50123.456);
    /// if let Some(rounded) = table.round(price) {
    ///     println!("Rounded price: {}", rounded);
    /// }
    /// ```
    pub fn round(&self, price: Decimal) -> Option<Decimal> {
        let tick = self.tick_for(price)?;
        // Use MidpointTowardZero strategy (round half dow)
        let rounded =
            (price / tick).round_dp_with_strategy(0, RoundingStrategy::MidpointTowardZero) * tick;
        Some(rounded)
    }

    /// Rounds a price to the nearest valid tick based on order side and conservativeness.
    ///
    /// This method allows directional rounding to favor either the maker or taker depending
    /// on the order side and whether you want to be conservative (safer pricing) or aggressive.
    ///
    /// # Parameters
    ///
    /// - `side`: The order side (Bid/Ask)
    /// - `price`: The price to round
    /// - `conservative`: If true, rounds in favor of maker (safer); if false, rounds in favor of taker (aggressive)
    ///
    /// # Rounding Behavior
    ///
    /// - **Ask + Conservative**: Rounds UP (higher sell price, safer for seller)
    /// - **Ask + Aggressive**: Rounds DOWN (lower sell price, more likely to fill)
    /// - **Bid + Conservative**: Rounds DOWN (lower buy price, safer for buyer)
    /// - **Bid + Aggressive**: Rounds UP (higher buy price, more likely to fill)
    ///
    /// Returns `None` if the price is invalid.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use hypersdk::hypercore::{PriceTickTable, types::Side};
    /// # use rust_decimal_macros::dec;
    /// let table = PriceTickTable { max_decimals: 1 };
    ///
    /// // Conservative ask: rounds up to safer (higher) price
    /// let price = table.round_by_side(Side::Ask, dec!(50123.4), true);
    /// // Returns Some(50124.0) - rounded up
    ///
    /// // Aggressive bid: rounds up to more likely fill
    /// let price = table.round_by_side(Side::Bid, dec!(50123.4), false);
    /// // Returns Some(50124.0) - rounded up
    /// ```
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

/// Creates a price tick table for a spot market.
///
/// See: <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/tick-and-lot-size>
fn build_price_ticks(sz_decimals: i64) -> PriceTickTable {
    let max_decimals = 8 - sz_decimals;
    PriceTickTable { max_decimals }
}

/// Creates a price tick table for a perpetual market.
///
/// For perps, the max significant figures is 5 and max decimal places is 6.
/// This function uses: max_decimals = 6 - sz_decimals (as a construction parameter)
///
/// See: <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/tick-and-lot-size>
fn build_perp_price_ticks(sz_decimals: i64) -> PriceTickTable {
    let max_decimals = 6 - sz_decimals;
    PriceTickTable { max_decimals }
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
    /// Price tick table for valid price increments
    pub table: PriceTickTable,
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
    /// Base token (tokens[0]) and quote token (tokens[1])
    pub tokens: [SpotToken; 2],
    /// Price tick table for valid price increments
    pub table: PriceTickTable,
}

impl SpotMarket {
    /// Returns the trading symbol in "BASE/QUOTE" format.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use hypersdk::hypercore::{SpotMarket, SpotToken, PriceTickTable};
    /// # let base = SpotToken {
    /// #     name: "BTC".into(), index: 0, token_id: Default::default(),
    /// #     evm_contract: None, cross_chain_address: None, sz_decimals: 8,
    /// #     wei_decimals: 8, evm_extra_decimals: 0
    /// # };
    /// # let quote = SpotToken {
    /// #     name: "USDC".into(), index: 1, token_id: Default::default(),
    /// #     evm_contract: None, cross_chain_address: None, sz_decimals: 6,
    /// #     wei_decimals: 6, evm_extra_decimals: 0
    /// # };
    /// # let market = SpotMarket {
    /// #     name: "BTC/USDC".into(), index: 10000, tokens: [base, quote],
    /// #     table: PriceTickTable { max_decimals: 0 }
    /// # };
    /// assert_eq!(market.symbol(), "BTC/USDC");
    /// ```
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

    /// Returns the price tick table for this market.
    #[must_use]
    pub fn tick_table(&self) -> &PriceTickTable {
        &self.table
    }

    /// Rounds a price to the nearest valid tick for this market.
    ///
    /// Returns `None` if the price is invalid.
    pub fn round_price(&self, price: Decimal) -> Option<Decimal> {
        self.table.round(price)
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
    use super::*;
    use rust_decimal::dec;

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
#[derive(Debug, Clone)]
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
    /// # use hypersdk::{Address, Decimal};
    /// # use rust_decimal_macros::dec;
    /// # let token = SpotToken {
    /// #     name: "USDC".into(), index: 0, token_id: Default::default(),
    /// #     evm_contract: None, cross_chain_address: None,
    /// #     sz_decimals: 6, wei_decimals: 6, evm_extra_decimals: 12
    /// # };
    /// let amount = dec!(100.50);
    /// let wei = token.to_wei(amount);
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

    let resp = client
        .post(url)
        .json(&serde_json::json!({
            "type": "spotMeta"
        }))
        .send()
        .await?;
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

/// Fetches all available perpetual futures markets from HyperCore.
///
/// Returns a list of all perpetual contracts with leverage, collateral, and margin information.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore;
///
/// # async fn example() -> anyhow::Result<()> {
/// let url = hypercore::mainnet_url();
/// let client = reqwest::Client::new();
/// let markets = hypercore::perp_markets(url, client).await?;
///
/// for market in markets {
///     println!("{}: {}x leverage", market.name, market.max_leverage);
/// }
/// # Ok(())
/// # }
/// ```
pub async fn perp_markets(
    core_url: impl IntoUrl,
    client: reqwest::Client,
) -> anyhow::Result<Vec<PerpMarket>> {
    let mut url = core_url.into_url()?;
    url.set_path("/info");

    // get it to gather the collateral token
    let spot = raw_spot_markets(url.clone(), client.clone()).await?;

    let resp = client
        .post(url)
        .json(&serde_json::json!({
            "type": "meta"
        }))
        .send()
        .await
        .context("info")?;
    let data: PerpTokens = resp.json().await?;
    let collateral = &spot.tokens[data.collateral_token];
    let collateral = SpotToken::from(collateral.clone());

    let perps = data
        .universe
        .into_iter()
        .enumerate()
        .map(|(index, perp)| PerpMarket {
            name: perp.name,
            index,
            max_leverage: perp.max_leverage,
            sz_decimals: perp.sz_decimals,
            collateral: collateral.clone(),
            isolated_margin: perp.only_isolated,
            margin_mode: perp.margin_mode,
            table: build_perp_price_ticks(perp.sz_decimals),
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
}
