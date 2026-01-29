//! HTTP client for HyperCore API interactions.
//!
//! This module provides the HTTP client for placing orders, querying balances,
//! managing positions, and performing asset transfers on Hyperliquid.
//!
//! # Examples
//!
//! ## Query User Balances
//!
//! ```no_run
//! use hypersdk::hypercore;
//! use hypersdk::Address;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let client = hypercore::mainnet();
//! let user: Address = "0x...".parse()?;
//! let balances = client.user_balances(user).await?;
//!
//! for balance in balances {
//!     println!("{}: {}", balance.coin, balance.total);
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
//! let signer: PrivateKeySigner = "your_key".parse()?;
//!
//! // Note: This example shows the structure but cannot run without
//! // the rust_decimal_macros::dec!() macro and chrono clock feature.
//! // In real usage, replace with actual decimal values and timestamp.
//! # Ok(())
//! # }
//! ```

use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};

use alloy::{
    primitives::Address,
    signers::{Signer, SignerSync},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use url::Url;

use super::signing::*;
use crate::hypercore::{
    ActionError, ApiAgent, CandleInterval, Chain, Cloid, Dex, MultiSigConfig, OidOrCloid,
    PerpMarket, Signature, SpotMarket, SpotToken,
    api::{
        Action, ActionRequest, ApproveAgent, ConvertToMultiSigUser, OkResponse, Response,
        SignersConfig,
    },
    mainnet_url, testnet_url,
    types::{
        BasicOrder, BatchCancel, BatchCancelCloid, BatchModify, BatchOrder, ClearinghouseState,
        Fill, FundingRate, InfoRequest, OrderResponseStatus, OrderUpdate, ScheduleCancel,
        SendAsset, SendToken, SpotSend, SubAccount, UsdSend, UserBalance, UserRole,
        UserVaultEquity, VaultDetails,
    },
};

/// HTTP client for HyperCore API.
///
/// Provides methods for trading, querying market data, managing positions,
/// and performing asset transfers.
///
/// # Example
///
/// ```
/// use hypersdk::hypercore;
///
/// let client = hypercore::mainnet();
/// // Use client for API calls
/// ```
pub struct Client {
    http_client: reqwest::Client,
    base_url: Url,
    chain: Chain,
}

impl Client {
    /// Creates a new HTTP client for the specified chain.
    ///
    /// The base URL is automatically determined based on the chain:
    /// - `Chain::Mainnet`: `https://api.hyperliquid.xyz`
    /// - `Chain::Testnet`: `https://api.hyperliquid-testnet.xyz`
    ///
    /// All actions signed by this client will use chain-specific values:
    /// - Agent source field: `"a"` for mainnet, `"b"` for testnet
    /// - Multisig chain ID: `"0x66eee"` for mainnet, `"0x66eef"` for testnet
    ///
    /// # Example
    ///
    /// ```
    /// use hypersdk::hypercore::{HttpClient, Chain};
    ///
    /// // Create a mainnet client
    /// let mainnet_client = HttpClient::new(Chain::Mainnet);
    ///
    /// // Create a testnet client
    /// let testnet_client = HttpClient::new(Chain::Testnet);
    /// ```
    pub fn new(chain: Chain) -> Self {
        let base_url = if chain.is_mainnet() {
            mainnet_url()
        } else {
            testnet_url()
        };

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .tcp_nodelay(true)
            .build()
            .unwrap();

        Self {
            http_client,
            base_url,
            chain,
        }
    }

    /// Sets a custom base URL for this client.
    ///
    /// This is useful when connecting to a custom Hyperliquid node or proxy.
    /// The chain configuration is preserved.
    ///
    /// # Example
    ///
    /// ```
    /// use hypersdk::hypercore::{HttpClient, Chain};
    /// use url::Url;
    ///
    /// let custom_url: Url = "https://my-custom-node.example.com".parse().unwrap();
    /// let client = HttpClient::new(Chain::Mainnet)
    ///     .with_url(custom_url);
    /// ```
    pub fn with_url(self, base_url: Url) -> Self {
        Self { base_url, ..self }
    }

    /// Returns the chain this client is configured for.
    #[must_use]
    pub const fn chain(&self) -> Chain {
        self.chain
    }

    /// Creates a WebSocket connection using the same base URL as this HTTP client.
    ///
    /// # Example
    ///
    /// ```
    /// use hypersdk::hypercore;
    /// use futures::StreamExt;
    ///
    /// # async fn example() {
    /// let client = hypercore::mainnet();
    /// let mut ws = client.websocket();
    /// // Subscribe and process messages
    /// # }
    /// ```
    pub fn websocket(&self) -> super::WebSocket {
        let mut url = self.base_url.clone();
        let _ = url.set_scheme("wss");
        url.set_path("/ws");
        super::WebSocket::new(url)
    }

    /// Creates a WebSocket connection without TLS (uses `ws://` instead of `wss://`).
    ///
    /// Useful for testing or local development.
    pub fn websocket_no_tls(&self) -> super::WebSocket {
        let mut url = self.base_url.clone();
        let _ = url.set_scheme("ws");
        url.set_path("/ws");
        super::WebSocket::new(url)
    }

    /// Fetches all available perpetual futures markets.
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
    ///     println!("{}: {}x leverage", market.name, market.max_leverage);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub async fn perps(&self) -> Result<Vec<PerpMarket>> {
        super::perp_markets(self.base_url.clone(), self.http_client.clone(), None).await
    }

    /// Fetches perpetual markets from a specific DEX.
    ///
    /// Returns a list of perpetual futures markets available on the specified DEX.
    /// Use this when you want to query markets from a specific exchange rather than
    /// the default Hyperliquid DEX.
    ///
    /// # Parameters
    ///
    /// - `dex`: The DEX to query markets from
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use hypersdk::hypercore;
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    ///
    /// // Get available DEXes
    /// let dexes = client.perp_dexs().await?;
    ///
    /// // Query markets from a specific DEX
    /// if let Some(dex) = dexes.first() {
    ///     let markets = client.perps_from(dex.clone()).await?;
    ///     for market in markets {
    ///         println!("{}: {}x leverage", market.name, market.max_leverage);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub async fn perps_from(&self, dex: Dex) -> Result<Vec<PerpMarket>> {
        super::perp_markets(self.base_url.clone(), self.http_client.clone(), Some(dex)).await
    }

    /// Fetches all available perpetual futures DEXes.
    ///
    /// Returns a list of all DEXes that offer perpetual futures trading on Hyperliquid.
    /// Each DEX has a unique name and index.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use hypersdk::hypercore;
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    /// let dexes = client.perp_dexs().await?;
    ///
    /// for dex in dexes {
    ///     println!("DEX: {}", dex.name());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub async fn perp_dexs(&self) -> Result<Vec<Dex>> {
        super::perp_dexs(self.base_url.clone(), self.http_client.clone()).await
    }

    /// Fetches all available spot markets.
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
    ///     println!("{}", market.symbol());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub async fn spot(&self) -> Result<Vec<SpotMarket>> {
        super::spot_markets(self.base_url.clone(), self.http_client.clone()).await
    }

    /// Fetches all available spot tokens.
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
    ///     println!("{}: {} decimals", token.name, token.sz_decimals);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub async fn spot_tokens(&self) -> Result<Vec<SpotToken>> {
        super::spot_tokens(self.base_url.clone(), self.http_client.clone()).await
    }

    /// Returns all open orders for a user.
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
    /// let orders = client.open_orders(user, None).await?;
    ///
    /// for order in orders {
    ///     println!("{} {} @ {}", order.side, order.sz, order.limit_px);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn open_orders(
        &self,
        user: Address,
        dex_name: Option<String>,
    ) -> Result<Vec<BasicOrder>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let data = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::FrontendOpenOrders {
                user,
                dex: dex_name,
            })
            .send()
            .await?
            .json()
            .await?;

        Ok(data)
    }

    /// Returns mid prices for all perpetual markets.
    ///
    /// Returns a map of market name to mid price.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    /// let mids = client.all_mids().await?;
    ///
    /// for (market, price) in mids {
    ///     println!("{}: {}", market, price);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn all_mids(&self, dex_name: Option<String>) -> Result<HashMap<String, Decimal>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let data = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::AllMids { dex: dex_name })
            .send()
            .await?
            .json()
            .await?;

        Ok(data)
    }

    /// Returns the user's historical orders.
    pub async fn historical_orders(&self, user: Address) -> Result<Vec<BasicOrder>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let data = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::HistoricalOrders { user })
            .send()
            .await?
            .json()
            .await?;

        Ok(data)
    }

    /// Returns the user's fills.
    pub async fn user_fills(&self, user: Address) -> Result<Vec<Fill>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let data = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::UserFills { user })
            .send()
            .await?
            .json()
            .await?;

        Ok(data)
    }

    /// Returns the status of an order.
    pub async fn order_status(
        &self,
        user: Address,
        oid: OidOrCloid,
    ) -> Result<Option<OrderUpdate>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        #[serde(tag = "status")]
        enum Response {
            Order { order: OrderUpdate },
            UnknownOid,
        }

        let data: Response = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::OrderStatus { user, oid })
            .send()
            .await?
            .json()
            .await?;

        Ok(match data {
            Response::Order { order } => Some(order),
            Response::UnknownOid => None,
        })
    }

    /// Returns historical candlestick data for a market.
    ///
    /// Retrieves OHLCV (Open, High, Low, Close, Volume) candlestick data for the specified
    /// market and time range. Only the most recent 5000 candles are available.
    ///
    /// # Parameters
    ///
    /// - `coin`: Market symbol (e.g., "BTC", "ETH"). For HIP-3 assets, prefix with dex name (e.g., "xyz:XYZ100")
    /// - `interval`: Candle interval (e.g., "1m", "15m", "1h", "1d")
    /// - `start_time`: Start time in milliseconds
    /// - `end_time`: End time in milliseconds
    ///
    /// # Available Intervals
    ///
    /// - Minutes: 1m, 3m, 5m, 15m, 30m
    /// - Hours: 1h, 2h, 4h, 8h, 12h
    /// - Days: 1d, 3d, 1w, 1M
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore::{self, CandleInterval};
    /// use chrono::{Utc, Duration};
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    ///
    /// // Get last 100 15-minute candles
    /// let end_time = Utc::now().timestamp_millis() as u64;
    /// let start_time = (Utc::now() - Duration::hours(25)).timestamp_millis() as u64;
    ///
    /// let candles = client
    ///     .candle_snapshot("BTC", CandleInterval::FifteenMinutes, start_time, end_time)
    ///     .await?;
    ///
    /// for candle in candles {
    ///     println!("BTC: O:{} H:{} L:{} C:{} V:{}",
    ///         candle.open, candle.high, candle.low, candle.close, candle.volume);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn candle_snapshot(
        &self,
        coin: impl Into<String>,
        interval: CandleInterval,
        start_time: u64,
        end_time: u64,
    ) -> Result<Vec<super::types::Candle>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let req = super::types::CandleSnapshotRequest {
            coin: coin.into(),
            interval,
            start_time,
            end_time,
        };

        let data = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::CandleSnapshot { req })
            .send()
            .await?
            .json()
            .await?;

        Ok(data)
    }

    /// Retrieves spot token balances for a user.
    ///
    /// Returns all tokens the user holds on the spot market, including held (locked) and total amounts.
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
    /// let balances = client.user_balances(user).await?;
    ///
    /// for balance in balances {
    ///     println!("{}: total={}, held={}", balance.coin, balance.total, balance.hold);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn user_balances(&self, user: Address) -> Result<Vec<UserBalance>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        #[derive(Deserialize)]
        struct Balances {
            balances: Vec<UserBalance>,
        }

        let data: Balances = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::SpotClearinghouseState { user })
            .send()
            .await?
            .json()
            .await?;

        Ok(data.balances)
    }

    /// Retrieves the clearinghouse state for a user's perpetual positions.
    ///
    /// Returns the complete state of a user's perpetual trading account, including
    /// margin summaries, withdrawable amounts, and all open positions.
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
    /// // Check account value and withdrawable amount
    /// println!("Account value: {}", state.margin_summary.account_value);
    /// println!("Withdrawable: {}", state.withdrawable);
    ///
    /// // Check margin utilization
    /// let utilization = state.margin_summary.margin_utilization();
    /// println!("Margin utilization: {}%", utilization);
    ///
    /// // Iterate through positions
    /// for asset_position in &state.asset_positions {
    ///     let pos = &asset_position.position;
    ///     println!("{} {}: {} @ {:?} (PnL: {})",
    ///         pos.side(),
    ///         pos.coin,
    ///         pos.szi,
    ///         pos.entry_px,
    ///         pos.unrealized_pnl
    ///     );
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn clearinghouse_state(
        &self,
        user: Address,
        dex_name: Option<String>,
    ) -> Result<ClearinghouseState> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let data = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::ClearinghouseState {
                user,
                dex: dex_name,
            })
            .send()
            .await?
            .json()
            .await?;
        Ok(data)
    }

    /// Retrieves historical funding rates for a perpetual market.
    ///
    /// Returns funding rate snapshots for the specified coin within the given time range.
    /// Funding rates are typically applied every 8 hours.
    ///
    /// # Parameters
    ///
    /// - `coin`: Market symbol (e.g., "BTC", "ETH")
    /// - `start_time`: Start time in milliseconds (inclusive)
    /// - `end_time`: Optional end time in milliseconds (inclusive). Defaults to current time.
    ///
    /// # Notes
    ///
    /// - Only the most recent 500 records are returned per request
    /// - To paginate, use the last returned timestamp as the next `start_time`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    ///
    /// // Get BTC funding rates from the last 24 hours
    /// let end_time = chrono::Utc::now().timestamp_millis() as u64;
    /// let start_time = end_time - 24 * 60 * 60 * 1000; // 24 hours ago
    ///
    /// let rates = client.funding_history("BTC", start_time, Some(end_time)).await?;
    ///
    /// for rate in rates {
    ///     println!("Funding rate at {}: {} (premium: {})",
    ///         rate.time, rate.funding_rate, rate.premium);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn funding_history(
        &self,
        coin: impl Into<String>,
        start_time: u64,
        end_time: Option<u64>,
    ) -> Result<Vec<FundingRate>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let data = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::FundingHistory {
                coin: coin.into(),
                start_time,
                end_time,
            })
            .send()
            .await?
            .json()
            .await?;

        Ok(data)
    }

    /// Retrieves the multi-signature wallet configuration for a user.
    ///
    /// Returns the list of authorized signers and the signature threshold required
    /// for executing transactions on behalf of a multisig account.
    ///
    /// # Arguments
    ///
    /// * `user` - The address of the multisig account to query
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    /// let multisig_addr = "0x1234567890abcdef1234567890abcdef12345678".parse()?;
    ///
    /// // Get multisig configuration
    /// let config = client.multi_sig_config(multisig_addr).await?;
    ///
    /// println!("Multisig requires {} of {} signatures",
    ///     config.threshold,
    ///     config.authorized_users.len()
    /// );
    ///
    /// for (i, signer) in config.authorized_users.iter().enumerate() {
    ///     println!("Authorized signer {}: {:?}", i + 1, signer);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn multi_sig_config(&self, user: Address) -> Result<MultiSigConfig> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let resp = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::UserToMultiSigSigners { user })
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// Get API agents for a user.
    ///
    /// Returns a list of additional agents authorized to act on behalf of the specified user.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore;
    /// use alloy::primitives::address;
    /// async fn example() -> anyhow::Result<()> {
    ///     let client = hypercore::mainnet();
    ///     let user = address!("0000000000000000000000000000000000000000");
    ///     let agents = client.api_agents(user).await?;
    ///
    ///     for agent in agents {
    ///         println!("Agent {}: {:?}, valid until: {:?}", agent.name, agent.address, agent.valid_until);
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn api_agents(&self, user: Address) -> Result<Vec<ApiAgent>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let resp = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::ExtraAgents { user })
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// Retrieve details for a vault.
    ///
    /// Returns comprehensive information about a vault including performance metrics,
    /// follower information, and configuration.
    ///
    /// # Parameters
    ///
    /// - `vault_address`: The address of the vault to query
    /// - `user`: Optional user address to include follower state for that user
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore;
    /// use hypersdk::Address;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    /// let vault: Address = "0xdfc24b077bc1425ad1dea75bcb6f8158e10df303".parse()?;
    ///
    /// // Get vault details without user-specific follower state
    /// let details = client.vault_details(vault, None).await?;
    /// println!("Vault: {} (APR: {}%)", details.name, details.apr);
    ///
    /// // Get vault details with user-specific follower state
    /// let user: Address = "0x...".parse()?;
    /// let details_with_state = client.vault_details(vault, Some(user)).await?;
    /// if let Some(state) = details_with_state.follower_state {
    ///     println!("Your equity: {}", state.vault_equity);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint#retrieve-details-for-a-vault>
    pub async fn vault_details(
        &self,
        vault_address: Address,
        user: Option<Address>,
    ) -> Result<VaultDetails> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let resp = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::VaultDetails {
                vault_address,
                user,
            })
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// Retrieve a user's vault deposits.
    ///
    /// Returns all vaults that a user has deposited into, along with their
    /// current equity in each vault.
    ///
    /// # Parameters
    ///
    /// - `user`: The address of the user to query vault deposits for
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
    ///
    /// let equities = client.user_vault_equities(user).await?;
    /// for equity in equities {
    ///     println!("Vault {:?}: equity = {}", equity.vault_address, equity.equity);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint#retrieve-a-users-vault-deposits>
    pub async fn user_vault_equities(&self, user: Address) -> Result<Vec<UserVaultEquity>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let resp = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::UserVaultEquities { user })
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// Query a user's role.
    ///
    /// Returns the role of an address in the Hyperliquid system. This can be used
    /// to determine if an address is a regular user, vault, agent, or subaccount.
    ///
    /// # Parameters
    ///
    /// - `user`: The address to query the role for
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
    ///
    /// let role = client.user_role(addr).await?;
    /// match role {
    ///     hypersdk::hypercore::types::UserRole::User => println!("Regular user"),
    ///     hypersdk::hypercore::types::UserRole::Vault => println!("Vault account"),
    ///     hypersdk::hypercore::types::UserRole::Agent { user } => {
    ///         println!("Agent wallet for {}", user);
    ///     }
    ///     hypersdk::hypercore::types::UserRole::SubAccount { master } => println!("Subaccount {master}"),
    ///     hypersdk::hypercore::types::UserRole::Missing => println!("Not found"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint#query-a-users-role>
    pub async fn user_role(&self, user: Address) -> Result<UserRole> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let resp = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::UserRole { user })
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// Retrieve a user's subaccounts.
    ///
    /// Returns all subaccounts associated with a master account, including their
    /// clearinghouse state (perpetuals) and spot balances.
    ///
    /// # Parameters
    ///
    /// - `user`: The address of the master account
    ///
    /// # Notes
    ///
    /// Subaccounts do not have private keys. To perform actions on behalf of a
    /// subaccount, signing should be done by the master account with the
    /// `vault_address` parameter set to the subaccount's address.
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
    ///     println!("  Withdrawable: {}", sub.clearinghouse_state.withdrawable);
    ///
    ///     // Check spot balances
    ///     for balance in &sub.spot_state.balances {
    ///         println!("  {}: {}", balance.coin, balance.total);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint#retrieve-a-users-subaccounts>
    pub async fn subaccounts(&self, user: Address) -> Result<Vec<SubAccount>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let resp = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::SubAccounts { user })
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }

    /// Schedule cancellation.
    pub async fn schedule_cancel<S: SignerSync>(
        &self,
        signer: &S,
        nonce: u64,
        when: DateTime<Utc>,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let resp = self
            .sign_and_send_sync(
                signer,
                ScheduleCancel {
                    time: Some(when.timestamp_millis() as u64),
                },
                nonce,
                vault_address,
                expires_after,
            )
            .await?;

        match resp {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => {
                anyhow::bail!("schedule_cancel: {err}")
            }
            _ => anyhow::bail!("schedule_cancel: unexpected response type: {resp:?}"),
        }
    }

    /// Places a batch of orders.
    ///
    /// Submits one or more orders to the exchange. Each order must be signed with your private key.
    ///
    /// # Parameters
    ///
    /// - `signer`: Private key signer for EIP-712 signatures
    /// - `batch`: Batch of orders to place
    /// - `nonce`: Unique nonce (typically current timestamp in milliseconds)
    /// - `vault_address`: Optional vault address if trading on behalf of a vault
    /// - `expires_after`: Optional expiration timestamp for the request
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore::{self, types::*, PrivateKeySigner};
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    /// let signer: PrivateKeySigner = "your_key".parse()?;
    ///
    /// // Example order placement - requires dec!() macro and timestamp
    /// // let order = BatchOrder { ... };
    /// // let nonce = chrono::Utc::now().timestamp_millis() as u64;
    /// // let statuses = client.place(&signer, order, nonce, None, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn place<S: SignerSync>(
        &self,
        signer: &S,
        batch: BatchOrder,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<Vec<OrderResponseStatus>, ActionError<Cloid>>> + Send + 'static
    {
        let cloids: Vec<_> = batch.orders.iter().map(|req| req.cloid).collect();

        let future = self.sign_and_send_sync(signer, batch, nonce, vault_address, expires_after);
        async move {
            let resp = future.await.map_err(|err| ActionError {
                ids: cloids.clone(),
                err: err.to_string(),
            })?;

            match resp {
                Response::Ok(OkResponse::Order { statuses }) => Ok(statuses),
                Response::Err(err) => Err(ActionError { ids: cloids, err }),
                _ => Err(ActionError {
                    ids: cloids,
                    err: format!("unexpected response type: {resp:?}"),
                }),
            }
        }
    }

    /// Cancel a batch of orders.
    pub fn cancel<S: SignerSync>(
        &self,
        signer: &S,
        batch: BatchCancel,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<Vec<OrderResponseStatus>, ActionError<u64>>> + Send + 'static
    {
        let oids: Vec<_> = batch.cancels.iter().map(|req| req.oid).collect();

        let future = self.sign_and_send_sync(signer, batch, nonce, vault_address, expires_after);

        async move {
            let resp = future.await.map_err(|err| ActionError {
                ids: oids.clone(),
                err: err.to_string(),
            })?;

            match resp {
                Response::Ok(OkResponse::Cancel { statuses }) => Ok(statuses),
                Response::Err(err) => Err(ActionError { ids: oids, err }),
                _ => Err(ActionError {
                    ids: oids,
                    err: format!("unexpected response type: {resp:?}"),
                }),
            }
        }
    }

    /// Cancel a batch of orders by cloid.
    pub fn cancel_by_cloid<S: SignerSync>(
        &self,
        signer: &S,
        batch: BatchCancelCloid,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<Vec<OrderResponseStatus>, ActionError<Cloid>>> + Send + 'static
    {
        let cloids: Vec<_> = batch.cancels.iter().map(|req| req.cloid).collect();

        let future = self.sign_and_send_sync(signer, batch, nonce, vault_address, expires_after);

        async move {
            let resp = future.await.map_err(|err| ActionError {
                ids: cloids.clone(),
                err: err.to_string(),
            })?;

            match resp {
                Response::Ok(OkResponse::Cancel { statuses }) => Ok(statuses),
                Response::Err(err) => Err(ActionError { ids: cloids, err }),
                _ => Err(ActionError {
                    ids: cloids,
                    err: format!("unexpected response type: {resp:?}"),
                }),
            }
        }
    }

    /// Modify a batch of orders.
    pub fn modify<S: SignerSync>(
        &self,
        signer: &S,
        batch: BatchModify,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<Vec<OrderResponseStatus>, ActionError<OidOrCloid>>> + Send + 'static
    {
        let cloids: Vec<_> = batch.modifies.iter().map(|req| req.oid).collect();

        let future = self.sign_and_send_sync(signer, batch, nonce, vault_address, expires_after);

        async move {
            let resp = future.await.map_err(|err| ActionError {
                ids: cloids.clone(),
                err: err.to_string(),
            })?;

            match resp {
                Response::Ok(OkResponse::Order { statuses }) => Ok(statuses),
                Response::Err(err) => Err(ActionError { ids: cloids, err }),
                _ => Err(ActionError {
                    ids: cloids,
                    err: format!("unexpected response type: {resp:?}"),
                }),
            }
        }
    }

    /// Approve a new agent.
    ///
    /// Approves an agent to act on behalf of the signer's account. An account can have:
    /// - 1 unnamed approved wallet
    /// - Up to 3 named agents
    /// - 2 named agents per subaccount
    ///
    /// # Parameters
    ///
    /// - `signer`: The wallet signing the approval
    /// - `agent`: The address of the agent to approve
    /// - `name`: The name for the agent (or empty string for unnamed)
    /// - `nonce`: The nonce for this action
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore;
    /// use alloy::primitives::address;
    /// use alloy::signers::local::PrivateKeySigner;
    ///
    /// async fn example() -> anyhow::Result<()> {
    ///     let client = hypercore::mainnet();
    ///     let signer = PrivateKeySigner::random();
    ///     let agent = address!("0x97271b6b7f3b23a2f4700ae671b05515ae5c3319");
    ///     let name = "my_agent".to_string();
    ///     let nonce = 123456789;
    ///
    ///     client.approve_agent(&signer, agent, name, nonce).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn approve_agent<S: Signer + Send + Sync>(
        &self,
        signer: &S,
        agent: Address,
        name: String,
        nonce: u64,
    ) -> Result<()> {
        let signature_chain_id = self.chain.arbitrum_id().to_owned();

        let approve_agent = ApproveAgent {
            signature_chain_id,
            hyperliquid_chain: self.chain,
            agent_address: agent,
            agent_name: if name.is_empty() { None } else { Some(name) },
            nonce,
        };

        let resp = self
            .sign_and_send(signer, approve_agent, nonce, None, None)
            .await?;
        match resp {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => {
                anyhow::bail!("approve_agent: {err}")
            }
            _ => anyhow::bail!("approve_agent: unexpected response type: {resp:?}"),
        }
    }

    /// Convert account to multi-signature user.
    ///
    /// Converts a regular account to a multisig account by specifying authorized signers
    /// and the required signature threshold. After conversion, the account will require
    /// multiple signatures to execute transactions.
    ///
    /// # Parameters
    ///
    /// - `signer`: The wallet signing the conversion (must be the account owner)
    /// - `authorized_users`: List of addresses authorized to sign for the multisig
    /// - `threshold`: Minimum number of signatures required (e.g., 2 for 2-of-3)
    /// - `nonce`: The nonce for this action
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore;
    /// use alloy::primitives::address;
    /// use alloy::signers::local::PrivateKeySigner;
    ///
    /// async fn example() -> anyhow::Result<()> {
    ///     let client = hypercore::mainnet();
    ///     let signer = PrivateKeySigner::random();
    ///     let authorized_users = vec![
    ///         address!("0x1111111111111111111111111111111111111111"),
    ///         address!("0x2222222222222222222222222222222222222222"),
    ///         address!("0x3333333333333333333333333333333333333333"),
    ///     ];
    ///     let threshold = 2; // 2-of-3 multisig
    ///     let nonce = 123456789;
    ///
    ///     client.convert_to_multisig(&signer, authorized_users, threshold, nonce).await?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn convert_to_multisig<S: Signer + Send + Sync>(
        &self,
        signer: &S,
        authorized_users: Vec<Address>,
        threshold: usize,
        nonce: u64,
    ) -> Result<()> {
        let chain = self.chain;
        let signature_chain_id = chain.arbitrum_id().to_owned();

        let convert = ConvertToMultiSigUser {
            signature_chain_id,
            hyperliquid_chain: chain,
            signers: SignersConfig {
                authorized_users,
                threshold,
            },
            nonce,
        };

        let resp = self
            .sign_and_send(signer, convert, nonce, None, None)
            .await?;
        match resp {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => {
                anyhow::bail!("convert_to_multisig: {err}")
            }
            _ => anyhow::bail!("convert_to_multisig: unexpected response type: {resp:?}"),
        }
    }

    /// Helper function to transfer from spot core to EVM.
    pub async fn transfer_to_evm<S: Send + SignerSync>(
        &self,
        signer: &S,
        token: SpotToken,
        amount: Decimal,
        nonce: u64,
    ) -> Result<()> {
        let destination = token
            .cross_chain_address
            .ok_or_else(|| anyhow::anyhow!("token {token} doesn't have a cross chain address"))?;

        self.spot_send(
            &signer,
            SpotSend {
                destination,
                token: SendToken(token),
                amount,
                time: nonce,
            },
            nonce,
        )
        .await
    }

    /// Helper function to transfer from perps to spot.
    ///
    /// Only USDC is accepted as `token`.
    pub async fn transfer_to_spot<S: Signer + SignerSync>(
        &self,
        signer: &S,
        token: SpotToken,
        amount: Decimal,
        nonce: u64,
    ) -> Result<()> {
        if token.name != "USDC" {
            return Err(anyhow::anyhow!(
                "only USDC is accepted, tried to transfer {}",
                token.name
            ));
        }

        self.send_asset(
            signer,
            SendAsset {
                destination: signer.address(),
                source_dex: "".into(),
                destination_dex: "spot".into(),
                token: SendToken(token),
                from_sub_account: "".into(),
                amount,
                nonce,
            },
            nonce,
        )
        .await
    }

    /// Helper function to transfer from spot to perps.
    ///
    /// Only USDC is accepted as `token`.
    pub async fn transfer_to_perps<S: Signer + SignerSync>(
        &self,
        signer: &S,
        token: SpotToken,
        amount: Decimal,
        nonce: u64,
    ) -> Result<()> {
        if token.name != "USDC" {
            return Err(anyhow::anyhow!(
                "only USDC is accepted, tried to transfer {}",
                token.name
            ));
        }

        self.send_asset(
            signer,
            SendAsset {
                destination: signer.address(),
                source_dex: "spot".into(),
                destination_dex: "".into(),
                token: SendToken(token),
                from_sub_account: "".into(),
                amount,
                nonce,
            },
            nonce,
        )
        .await
    }

    /// Send USDC to another address.
    ///
    /// Perp <> Perp transfers.
    ///
    /// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#core-usdc-transfer>
    pub async fn send_usdc<S: SignerSync>(
        &self,
        signer: &S,
        send: UsdSend,
        nonce: u64,
    ) -> Result<()> {
        let resp = self
            .sign_and_send_sync(signer, send.into_action(self.chain), nonce, None, None)
            .await?;
        match resp {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => {
                anyhow::bail!("send_usdc: {err}")
            }
            _ => anyhow::bail!("send_usdc: unexpected response type: {resp:?}"),
        }
    }

    /// Send USDC to another address.
    ///
    /// Spot <> DEX or Subaccount.
    ///
    /// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#send-asset>
    pub fn send_asset<S: SignerSync>(
        &self,
        signer: &S,
        send: SendAsset,
        nonce: u64,
    ) -> impl Future<Output = Result<()>> + Send + 'static {
        let future =
            self.sign_and_send_sync(signer, send.into_action(self.chain), nonce, None, None);

        async move {
            let resp = future.await?;
            match resp {
                Response::Ok(OkResponse::Default) => Ok(()),
                Response::Err(err) => {
                    anyhow::bail!("send_asset: {err}")
                }
                _ => anyhow::bail!("send_asset: unexpected response type: {resp:?}"),
            }
        }
    }

    /// Spot transfer.
    ///
    /// Spot <> Spot.
    ///
    /// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint#core-spot-transfer>
    pub fn spot_send<S: SignerSync>(
        &self,
        signer: &S,
        send: SpotSend,
        nonce: u64,
    ) -> impl Future<Output = Result<()>> + Send + 'static {
        let future =
            self.sign_and_send_sync(signer, send.into_action(self.chain), nonce, None, None);

        async move {
            let resp = future.await?;
            match resp {
                Response::Ok(OkResponse::Default) => Ok(()),
                Response::Err(err) => {
                    anyhow::bail!("spot send: {err}")
                }
                _ => anyhow::bail!("spot_send: unexpected response type: {resp:?}"),
            }
        }
    }

    /// Toggle big blocks or not idk.
    pub async fn evm_user_modify<S: SignerSync>(
        &self,
        signer: &S,
        toggle: bool,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let resp = self
            .sign_and_send_sync(
                signer,
                Action::EvmUserModify {
                    using_big_blocks: toggle,
                },
                nonce,
                vault_address,
                expires_after,
            )
            .await?;

        match resp {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => {
                anyhow::bail!("evm_user_modify: {err}")
            }
            _ => anyhow::bail!("evm_user_modify: unexpected response type: {resp:?}"),
        }
    }

    /// Invalidate a nonce.
    pub async fn noop<S: SignerSync>(
        &self,
        signer: &S,
        nonce: u64,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let resp = self
            .sign_and_send_sync(signer, Action::Noop, nonce, vault_address, expires_after)
            .await?;

        match resp {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => {
                anyhow::bail!("noop: {err}")
            }
            _ => anyhow::bail!("noop: unexpected response type: {resp:?}"),
        }
    }

    /// Executes a multisig action on Hyperliquid.
    ///
    /// This method allows multiple signers to authorize a single action (such as placing orders,
    /// canceling orders, or transferring funds) from a multisig wallet. All provided signers must
    /// be authorized on the multisig wallet configuration.
    ///
    /// # Parameters
    ///
    /// - `lead`: The lead signer who submits the transaction to the exchange
    /// - `multi_sig_user`: The multisig wallet address that will execute the action
    /// - `signers`: Iterator of all signers whose signatures are required (typically includes the lead)
    /// - `action`: The action to execute (Order, Cancel, Transfer, etc.)
    /// - `nonce`: Unique nonce for this transaction (typically current timestamp in milliseconds)
    ///
    /// # Multisig Process
    ///
    /// 1. The action is hashed with the multisig address and lead signer
    /// 2. Each signer signs the action hash using their private key
    /// 3. All signatures are collected into a multisig payload
    /// 4. The lead signer signs the entire multisig payload
    /// 5. The signed multisig transaction is submitted to the exchange
    /// 6. The exchange verifies all signatures match the multisig wallet's authorized signers
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore::{self, types::*, PrivateKeySigner};
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    ///
    /// // Parse the signers for the multisig wallet
    /// let signer1: PrivateKeySigner = "key1".parse()?;
    /// let signer2: PrivateKeySigner = "key2".parse()?;
    ///
    /// // The multisig wallet address
    /// let multisig_addr: hypersdk::Address = "0x...".parse()?;
    ///
    /// // Execute multisig operations - requires dec!() macro and timestamp
    /// // let nonce = chrono::Utc::now().timestamp_millis() as u64;
    /// // let response = client.multi_sig(&signer1, multisig_addr, nonce)
    /// //     .signer(&signer2)
    /// //     .place(order, None, None)
    /// //     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn multi_sig<'a, S: Signer + Send + Sync>(
        &'a self,
        lead: &'a S,
        multi_sig_user: Address,
        nonce: u64,
    ) -> MultiSig<'a, S> {
        MultiSig {
            lead,
            multi_sig_user,
            signers: VecDeque::new(),
            signatures: VecDeque::new(),
            client: self,
            nonce,
        }
    }

    /// Send a signed action hashing.
    fn sign_and_send_sync<S: SignerSync, A: Into<Action>>(
        &self,
        signer: &S,
        action: A,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        maybe_expires_after: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<Response>> + Send + 'static {
        let action: Action = action.into();
        let res = action.sign_sync(
            signer,
            nonce,
            maybe_vault_address,
            maybe_expires_after,
            self.chain,
        );

        let http_client = self.http_client.clone();
        let mut url = self.base_url.clone();
        url.set_path("/exchange");

        async move {
            let req = res?;
            let res = http_client
                .post(url)
                .timeout(Duration::from_secs(5))
                // .header(header::CONTENT_TYPE, "application/json")
                // .body(text)
                .json(&req)
                .send()
                .await?
                .json()
                // .text()
                .await?;
            // println!("<< {res}");
            // Ok(serde_json::from_str(&res).unwrap())
            Ok(res)
        }
    }

    /// Send a signed action hashing.
    async fn sign_and_send<S: Signer + Send + Sync, A: Into<Action>>(
        &self,
        signer: &S,
        action: A,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        maybe_expires_after: Option<DateTime<Utc>>,
    ) -> Result<Response> {
        let action: Action = action.into();
        let req = action
            .sign(
                signer,
                nonce,
                maybe_vault_address,
                maybe_expires_after,
                self.chain,
            )
            .await?;

        self.send(req).await
    }

    #[doc(hidden)]
    pub async fn send(&self, req: ActionRequest) -> Result<Response> {
        let http_client = self.http_client.clone();
        let mut url = self.base_url.clone();
        url.set_path("/exchange");

        let res = http_client
            .post(url)
            .timeout(Duration::from_secs(5))
            // .header(header::CONTENT_TYPE, "application/json")
            // .body(text)
            .json(&req)
            .send()
            .await?
            .json()
            .await?;
        Ok(res)
    }

    // TODO: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint#retrieve-a-users-subaccounts
}

/// Builder for constructing and executing multisig transactions on Hyperliquid.
///
/// The `MultiSig` struct provides a fluent API for building multisig transactions that require
/// multiple signers to authorize actions. It collects signatures from all required signers and
/// submits the complete multisig transaction to the exchange.
///
/// # Multisig Flow
///
/// 1. Create a `MultiSig` instance via `Client::multi_sig()`
/// 2. Add signers using `signer()` or `signers()`
/// 3. Execute an action (e.g., `place()`, `send_usdc()`)
/// 4. The builder collects signatures from all signers
/// 5. The lead signer submits the transaction
///
/// # Type Parameters
///
/// - `'a`: Lifetime of the client and signer references
/// - `S`: The signer type implementing `SignerSync + Signer`
///
/// # Example
///
/// ```rust,ignore
/// use hypersdk::hypercore::Client;
/// use alloy::signers::local::PrivateKeySigner;
///
/// let client = Client::mainnet();
/// let lead_signer: PrivateKeySigner = "0x...".parse()?;
/// let signer2: PrivateKeySigner = "0x...".parse()?;
/// let signer3: PrivateKeySigner = "0x...".parse()?;
/// let multisig_address = "0x...".parse()?;
/// let nonce = chrono::Utc::now().timestamp_millis() as u64;
///
/// // Execute a multisig order
/// let response = client
///     .multi_sig(&lead_signer, multisig_address, nonce)
///     .signer(&signer2)
///     .signer(&signer3)
///     .place(order, None, None)
///     .await?;
/// ```
///
/// # Notes
///
/// - The lead signer is the one who submits the transaction but also signs it
/// - All signers (including lead) must be authorized on the multisig wallet
/// - The order of signers should match the wallet's configuration
/// - Nonce must be unique for each transaction (typically millisecond timestamp)
pub struct MultiSig<'a, S: Signer + Send + Sync> {
    lead: &'a S,
    multi_sig_user: Address,
    signers: VecDeque<&'a S>,
    signatures: VecDeque<Signature>,
    nonce: u64,
    client: &'a Client,
}

impl<'a, S> MultiSig<'a, S>
where
    S: Signer + Send + Sync,
{
    /// Add a single signer to the multisig transaction.
    ///
    /// This method adds one signer to the list of signers who will authorize the transaction.
    /// You can chain multiple calls to add multiple signers.
    ///
    /// # Parameters
    ///
    /// - `signer`: A reference to the signer to add
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// client
    ///     .multi_sig(&lead, multisig_addr, nonce)
    ///     .signer(&signer1)
    ///     .signer(&signer2)
    ///     .signer(&signer3)
    ///     .place(order, None, None)
    ///     .await?;
    /// ```
    pub fn signer(mut self, signer: &'a S) -> Self {
        self.signers.push_back(signer);
        self
    }

    /// Add multiple signers to the multisig transaction.
    ///
    /// This method adds a collection of signers at once. More convenient than calling
    /// `signer()` multiple times when you have signers in a collection.
    ///
    /// # Parameters
    ///
    /// - `signers`: An iterable collection of signer references
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let signers = vec![&signer1, &signer2, &signer3];
    ///
    /// client
    ///     .multi_sig(&lead, multisig_addr, nonce)
    ///     .signers(signers)
    ///     .place(order, None, None)
    ///     .await?;
    /// ```
    pub fn signers(mut self, signers: impl IntoIterator<Item = &'a S>) -> Self {
        self.signers.extend(signers);
        self
    }

    /// Append pre-existing signatures to the multisig transaction.
    ///
    /// This method allows you to include signatures that were already collected separately,
    /// rather than generating them from signers. This is useful when:
    /// - Signatures were collected offline or asynchronously
    /// - You're aggregating signatures from multiple sources
    /// - You have a partial multisig that needs additional signatures
    ///
    /// The signatures should be in the same order and format as expected by the multisig
    /// wallet configuration.
    ///
    /// # Parameters
    ///
    /// - `signatures`: An iterable collection of pre-existing signatures
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hypersdk::hypercore::types::Signature;
    ///
    /// // Signatures collected from external sources
    /// let existing_sigs: Vec<Signature> = vec![
    ///     sig1, sig2, sig3
    /// ];
    ///
    /// client
    ///     .multi_sig(&lead, multisig_addr, nonce)
    ///     .signatures(existing_sigs)
    ///     .signer(&additional_signer)  // Can still add more signers
    ///     .place(order, None, None)
    ///     .await?;
    /// ```
    ///
    /// # Notes
    ///
    /// - Signatures are appended in order to the signature list
    /// - You can mix pre-existing signatures with new signers
    /// - Ensure signatures match the action being signed and the multisig configuration
    /// - The total number of signatures must match the multisig threshold
    pub fn signatures(mut self, signatures: impl IntoIterator<Item = Signature>) -> Self {
        self.signatures.extend(signatures);
        self
    }

    /// Place orders using the multisig account.
    ///
    /// This method collects signatures from all signers for a batch order placement using
    /// RMP (MessagePack) hashing, then submits the multisig transaction to the exchange.
    ///
    /// # Process
    ///
    /// 1. Creates an RMP hash of the order action
    /// 2. Each signer signs the hash using EIP-712
    /// 3. Collects all signatures into a `MultiSigAction`
    /// 4. Lead signer submits the complete transaction
    ///
    /// # Parameters
    ///
    /// - `batch`: The batch order to place
    /// - `vault_address`: Optional vault address if trading on behalf of a vault
    /// - `expires_after`: Optional expiration time for the request
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hypersdk::hypercore::types::{BatchOrder, OrderRequest, OrderTypePlacement, TimeInForce};
    /// use rust_decimal::dec;
    ///
    /// let order = OrderRequest {
    ///     asset: 0,
    ///     is_buy: true,
    ///     limit_px: dec!(50000),
    ///     sz: dec!(0.1),
    ///     reduce_only: false,
    ///     order_type: OrderTypePlacement::Limit {
    ///         tif: TimeInForce::Gtc,
    ///     },
    ///     cloid: [0u8; 16].into(),
    /// };
    ///
    /// let batch = BatchOrder {
    ///     orders: vec![order],
    ///     grouping: OrderGrouping::Na,
    /// };
    ///
    /// let statuses = client
    ///     .multi_sig(&lead, multisig_addr, nonce)
    ///     .signers(&signers)
    ///     .place(batch, None, None)
    ///     .await?;
    ///
    /// for status in statuses {
    ///     match status {
    ///         OrderResponseStatus::Resting { oid, .. } => {
    ///             println!("Order {} placed", oid);
    ///         }
    ///         OrderResponseStatus::Error(err) => {
    ///             eprintln!("Order failed: {}", err);
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    pub async fn place(
        &self,
        batch: BatchOrder,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
    ) -> Result<Vec<OrderResponseStatus>, ActionError<Cloid>> {
        let cloids: Vec<_> = batch.orders.iter().map(|req| req.cloid).collect();

        let action = multisig_collect_signatures(
            self.lead.address(),
            self.multi_sig_user,
            self.signers.iter().copied(),
            self.signatures.iter().copied(),
            Action::Order(batch),
            self.nonce,
            self.client.chain,
        )
        .await
        .map_err(|err| ActionError {
            ids: cloids.clone(),
            err: err.to_string(),
        })?;

        let resp = self
            .client
            .sign_and_send(self.lead, action, self.nonce, vault_address, expires_after)
            .await
            .map_err(|err| ActionError {
                ids: cloids.clone(),
                err: err.to_string(),
            })?;

        match resp {
            Response::Ok(OkResponse::Order { statuses }) => Ok(statuses),
            Response::Err(err) => Err(ActionError { ids: cloids, err }),
            _ => Err(ActionError {
                ids: cloids,
                err: format!("unexpected response type: {resp:?}"),
            }),
        }
    }

    /// Send USDC from the multisig account.
    ///
    /// This method collects signatures from all signers for a USDC transfer using EIP-712
    /// typed data, then submits the multisig transaction to the exchange.
    ///
    /// # Process
    ///
    /// 1. Creates EIP-712 typed data from the UsdSend action
    /// 2. Each signer signs the typed data directly using EIP-712
    /// 3. Collects all signatures into a `MultiSigAction`
    /// 4. Lead signer submits the complete transaction
    ///
    /// # Parameters
    ///
    /// - `send`: The UsdSend parameters (destination, amount, time, chain, etc.)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hypersdk::hypercore::types::{UsdSend, Chain};
    /// use hypersdk::hypercore::ARBITRUM_SIGNATURE_CHAIN_ID;
    /// use rust_decimal::dec;
    ///
    /// let send = UsdSend {
    ///     hyperliquid_chain: Chain::Mainnet,
    ///     signature_chain_id: ARBITRUM_SIGNATURE_CHAIN_ID,
    ///     destination: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb".parse()?,
    ///     amount: dec!(100),
    ///     time: chrono::Utc::now().timestamp_millis() as u64,
    /// };
    ///
    /// client
    ///     .multi_sig(&lead_signer, multisig_address, nonce)
    ///     .signers(&signers)
    ///     .send_usdc(send)
    ///     .await?;
    ///
    /// println!("Successfully sent 100 USDC from multisig account");
    /// ```
    ///
    /// # Notes
    ///
    /// - Uses EIP-712 typed data signatures (different from order placement which uses RMP)
    /// - Time should typically be the current timestamp in milliseconds
    /// - Destination can be any valid Ethereum address
    /// - Amount is in USDC (6 decimals on-chain, but use regular decimal representation)
    pub async fn send_usdc(&self, send: UsdSend) -> Result<()> {
        let nonce = send.time;
        let action = multisig_collect_signatures(
            self.lead.address(),
            self.multi_sig_user,
            self.signers.iter().copied(),
            self.signatures.iter().copied(),
            send.into_action(self.client.chain()).into(),
            nonce,
            self.client.chain,
        )
        .await?;

        let resp = self
            .client
            .sign_and_send(self.lead, action, self.nonce, None, None)
            .await?;

        match resp {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => anyhow::bail!("send_usdc: {err}"),
            _ => anyhow::bail!("send_usdc: unexpected response type: {resp:?}"),
        }
    }

    /// Send assets from the multisig account.
    ///
    /// This method collects signatures from all signers for an asset transfer using EIP-712
    /// typed data, then submits the multisig transaction to the exchange. This can be used
    /// to transfer assets between different destinations (accounts, DEXes, subaccounts).
    ///
    /// # Process
    ///
    /// 1. Creates EIP-712 typed data from the SendAsset action
    /// 2. Each signer signs the typed data directly using EIP-712
    /// 3. Collects all signatures into a `MultiSigAction`
    /// 4. Lead signer submits the complete transaction
    ///
    /// # Parameters
    ///
    /// - `send`: The SendAsset parameters (destination, token, amount, source/dest DEX, etc.)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hypersdk::hypercore::types::{SendAsset, SendToken};
    /// use hypersdk::hypercore::ARBITRUM_MAINNET_CHAIN_ID;
    /// use rust_decimal::dec;
    ///
    /// // Get the token info first
    /// let tokens = client.spot_meta().await?;
    /// let usdc = tokens.iter().find(|t| t.name == "USDC").unwrap();
    ///
    /// let send = SendAsset {
    ///     hyperliquid_chain: Chain::Mainnet,
    ///     signature_chain_id: ARBITRUM_MAINNET_CHAIN_ID,
    ///     destination: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb".parse()?,
    ///     source_dex: "".to_string(),      // Empty for perp balance
    ///     destination_dex: "".to_string(), // Empty for recipient's perp balance
    ///     token: SendToken(usdc.clone()),
    ///     from_sub_account: "".to_string(), // Empty for main account
    ///     amount: dec!(100),
    ///     nonce: chrono::Utc::now().timestamp_millis() as u64,
    /// };
    ///
    /// client
    ///     .multi_sig(&lead_signer, multisig_address, nonce)
    ///     .signers(&signers)
    ///     .send_asset(send)
    ///     .await?;
    ///
    /// println!("Successfully sent 100 USDC from multisig account");
    /// ```
    ///
    /// # Notes
    ///
    /// - Uses EIP-712 typed data signatures (different from order placement which uses RMP)
    /// - Source/destination DEX can be: "" (perp balance), "spot", or other DEX identifiers
    /// - Token must be obtained from `spot_meta()` API call
    /// - Nonce should be unique for each transaction (typically current timestamp in ms)
    pub async fn send_asset(&self, send: SendAsset) -> Result<()> {
        let nonce = send.nonce;
        let action = multisig_collect_signatures(
            self.lead.address(),
            self.multi_sig_user,
            self.signers.iter().copied(),
            self.signatures.iter().copied(),
            send.into_action(self.client.chain()).into(),
            nonce,
            self.client.chain,
        )
        .await?;

        let resp = self
            .client
            .sign_and_send(self.lead, action, self.nonce, None, None)
            .await?;

        match resp {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => anyhow::bail!("send_asset: {err}"),
            _ => anyhow::bail!("send_asset: unexpected response type: {resp:?}"),
        }
    }

    /// Approve a new agent for the multisig account.
    ///
    /// Approves an agent to act on behalf of the multisig account. An account can have:
    /// - 1 unnamed approved wallet
    /// - Up to 3 named agents
    /// - 2 named agents per subaccount
    ///
    /// # Parameters
    ///
    /// - `agent`: The address of the agent to approve
    /// - `name`: The name for the agent (or empty string for unnamed)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let agent = address!("0x97271b6b7f3b23a2f4700ae671b05515ae5c3319");
    /// let name = "my_agent".to_string();
    ///
    /// client
    ///     .multi_sig(&lead, multisig_addr, nonce)
    ///     .signer(&signer1)
    ///     .signer(&signer2)
    ///     .approve_agent(agent, name)
    ///     .await?;
    /// ```
    pub async fn approve_agent(&self, agent: Address, name: String) -> Result<()> {
        let chain = self.client.chain;
        let signature_chain_id = chain.arbitrum_id().to_owned();

        let approve_agent = ApproveAgent {
            signature_chain_id,
            hyperliquid_chain: chain,
            agent_address: agent,
            agent_name: if name.is_empty() { None } else { Some(name) },
            nonce: self.nonce,
        };

        let action = multisig_collect_signatures(
            self.lead.address(),
            self.multi_sig_user,
            self.signers.iter().copied(),
            self.signatures.iter().copied(),
            Action::ApproveAgent(approve_agent),
            self.nonce,
            self.client.chain,
        )
        .await?;

        let resp = self
            .client
            .sign_and_send(self.lead, action, self.nonce, None, None)
            .await?;

        match resp {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => anyhow::bail!("approve_agent: {err}"),
            _ => anyhow::bail!("approve_agent: unexpected response type: {resp:?}"),
        }
    }

    /// Convert multisig account back to normal user.
    ///
    /// Converts the multisig account back to a regular single-signer account by setting
    /// the signers to null. After conversion, the account will only require a single
    /// signature to execute transactions.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// client
    ///     .multi_sig(&lead, multisig_addr, nonce)
    ///     .signer(&signer1)
    ///     .signer(&signer2)
    ///     .convert_to_normal_user()
    ///     .await?;
    /// ```
    pub async fn convert_to_normal_user(&self) -> Result<()> {
        let chain = self.client.chain;

        let convert = ConvertToMultiSigUser {
            signature_chain_id: chain.arbitrum_id().to_owned(),
            hyperliquid_chain: chain,
            signers: SignersConfig {
                authorized_users: vec![], // Empty vec serializes to "null"
                threshold: 0,
            },
            nonce: self.nonce,
        };

        let action = multisig_collect_signatures(
            self.lead.address(),
            self.multi_sig_user,
            self.signers.iter().copied(),
            self.signatures.iter().copied(),
            Action::ConvertToMultiSigUser(convert),
            self.nonce,
            self.client.chain,
        )
        .await?;

        let resp = self
            .client
            .sign_and_send(self.lead, action, self.nonce, None, None)
            .await?;

        match resp {
            Response::Ok(OkResponse::Default) => Ok(()),
            Response::Err(err) => anyhow::bail!("convert_to_normal_user: {err}"),
            _ => anyhow::bail!("convert_to_normal_user: unexpected response type: {resp:?}"),
        }
    }
}
