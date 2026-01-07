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
//! use rust_decimal_macros::dec;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let client = hypercore::mainnet();
//! let signer: PrivateKeySigner = "your_key".parse()?;
//!
//! let order = BatchOrder {
//!     orders: vec![OrderRequest {
//!         asset: 0,
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

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    time::Duration,
};

use alloy::{
    primitives::Address,
    signers::{Signer, SignerSync},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use url::Url;

use super::signing::*;
use crate::hypercore::{
    Chain, Cloid, OidOrCloid, PerpMarket, SpotMarket, SpotToken, mainnet_url, testnet_url,
    types::{
        Action, ApiResponse, BasicOrder, BatchCancel, BatchCancelCloid, BatchModify, BatchOrder,
        Fill, OkResponse, OrderResponseStatus, OrderUpdate, ScheduleCancel, SendAsset, SendToken,
        SpotSend, UsdSend, UserBalance,
    },
};

/// Error type for batch operations that failed.
///
/// Contains the IDs of the orders/actions that failed and the error message.
///
/// # Type Parameter
///
/// - `T`: The ID type (e.g., `Cloid`, `u64`, `OidOrCloid`)
#[derive(Debug, Clone)]
pub struct ActionError<T> {
    /// The IDs of orders/actions that encountered the error
    ids: Vec<T>,
    /// The error message from the exchange
    err: String,
}

impl<T> ActionError<T> {
    /// Creates a new ActionError.
    pub fn new(ids: Vec<T>, err: String) -> Self {
        Self { ids, err }
    }

    /// Returns the error message.
    pub fn message(&self) -> &str {
        &self.err
    }

    /// Returns the failed IDs.
    pub fn ids(&self) -> &[T] {
        &self.ids
    }

    /// Consumes the error and returns the IDs.
    pub fn into_ids(self) -> Vec<T> {
        self.ids
    }
}

impl<T> fmt::Display for ActionError<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, ids: {:?}", self.err, self.ids)
    }
}

impl<T> std::error::Error for ActionError<T> where T: fmt::Display + fmt::Debug {}

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
        super::perp_markets(self.base_url.clone(), self.http_client.clone()).await
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
    /// let orders = client.open_orders(user).await?;
    ///
    /// for order in orders {
    ///     println!("{} {} @ {}", order.side, order.sz, order.limit_px);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn open_orders(&self, user: Address) -> Result<Vec<BasicOrder>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let data = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::FrontendOpenOrders { user })
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
    pub async fn all_mids(&self) -> Result<HashMap<String, Decimal>> {
        let mut api_url = self.base_url.clone();
        api_url.set_path("/info");

        let data = self
            .http_client
            .post(api_url)
            .json(&InfoRequest::AllMids)
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
            .sign_and_send(
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
            ApiResponse::Ok(OkResponse::Default) => Ok(()),
            ApiResponse::Err(err) => {
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
    /// # Returns
    ///
    /// A future that resolves to order statuses or an error.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore::{self, types::*, PrivateKeySigner};
    /// use rust_decimal_macros::dec;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    /// let signer: PrivateKeySigner = "your_key".parse()?;
    ///
    /// let order = BatchOrder {
    ///     orders: vec![OrderRequest {
    ///         asset: 0,
    ///         is_buy: true,
    ///         limit_px: dec!(50000),
    ///         sz: dec!(0.1),
    ///         reduce_only: false,
    ///         order_type: OrderTypePlacement::Limit {
    ///             tif: TimeInForce::Gtc,
    ///         },
    ///         cloid: Default::default(),
    ///     }],
    ///     grouping: OrderGrouping::Na,
    /// };
    ///
    /// let nonce = chrono::Utc::now().timestamp_millis() as u64;
    /// let statuses = client.place(&signer, order, nonce, None, None).await?;
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

        let future = self.sign_and_send(signer, batch, nonce, vault_address, expires_after);

        async move {
            let resp = future.await.map_err(|err| ActionError {
                ids: cloids.clone(),
                err: err.to_string(),
            })?;

            match resp {
                ApiResponse::Ok(OkResponse::Order { statuses }) => Ok(statuses),
                ApiResponse::Err(err) => Err(ActionError { ids: cloids, err }),
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

        let future = self.sign_and_send(signer, batch, nonce, vault_address, expires_after);

        async move {
            let resp = future.await.map_err(|err| ActionError {
                ids: oids.clone(),
                err: err.to_string(),
            })?;

            match resp {
                ApiResponse::Ok(OkResponse::Order { statuses }) => Ok(statuses),
                ApiResponse::Err(err) => Err(ActionError { ids: oids, err }),
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

        let future = self.sign_and_send(signer, batch, nonce, vault_address, expires_after);

        async move {
            let resp = future.await.map_err(|err| ActionError {
                ids: cloids.clone(),
                err: err.to_string(),
            })?;

            match resp {
                ApiResponse::Ok(OkResponse::Order { statuses }) => Ok(statuses),
                ApiResponse::Err(err) => Err(ActionError { ids: cloids, err }),
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

        let future = self.sign_and_send(signer, batch, nonce, vault_address, expires_after);

        async move {
            let resp = future.await.map_err(|err| ActionError {
                ids: cloids.clone(),
                err: err.to_string(),
            })?;

            match resp {
                ApiResponse::Ok(OkResponse::Order { statuses }) => Ok(statuses),
                ApiResponse::Err(err) => Err(ActionError { ids: cloids, err }),
                _ => Err(ActionError {
                    ids: cloids,
                    err: format!("unexpected response type: {resp:?}"),
                }),
            }
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
            .sign_and_send(
                signer,
                send.into_action(self.chain.arbitrum_id(), self.chain),
                nonce,
                None,
                None,
            )
            .await?;
        match resp {
            ApiResponse::Ok(OkResponse::Default) => Ok(()),
            ApiResponse::Err(err) => {
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
        let future = self.sign_and_send(
            signer,
            send.into_action(self.chain.arbitrum_id(), self.chain),
            nonce,
            None,
            None,
        );

        async move {
            let resp = future.await?;
            match resp {
                ApiResponse::Ok(OkResponse::Default) => Ok(()),
                ApiResponse::Err(err) => {
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
        let future = self.sign_and_send(
            signer,
            send.into_action(self.chain.arbitrum_id(), self.chain),
            nonce,
            None,
            None,
        );

        async move {
            let resp = future.await?;
            match resp {
                ApiResponse::Ok(OkResponse::Default) => Ok(()),
                ApiResponse::Err(err) => {
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
            .sign_and_send(
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
            ApiResponse::Ok(OkResponse::Default) => Ok(()),
            ApiResponse::Err(err) => {
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
            .sign_and_send(signer, Action::Noop, nonce, vault_address, expires_after)
            .await?;

        match resp {
            ApiResponse::Ok(OkResponse::Default) => Ok(()),
            ApiResponse::Err(err) => {
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
    /// # Returns
    ///
    /// Returns the API response containing the result of the action execution.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hypersdk::hypercore::{self, types::*, PrivateKeySigner};
    /// use rust_decimal_macros::dec;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let client = hypercore::mainnet();
    ///
    /// // Parse the signers for the multisig wallet
    /// let signer1: PrivateKeySigner = "key1".parse()?;
    /// let signer2: PrivateKeySigner = "key2".parse()?;
    /// let signers = vec![&signer1, &signer2];
    ///
    /// // The multisig wallet address
    /// let multisig_addr: hypersdk::Address = "0x...".parse()?;
    ///
    /// // Create an order action
    /// let order = BatchOrder {
    ///     orders: vec![OrderRequest {
    ///         asset: 0,
    ///         is_buy: true,
    ///         limit_px: dec!(50000),
    ///         sz: dec!(0.1),
    ///         reduce_only: false,
    ///         order_type: OrderTypePlacement::Limit {
    ///             tif: TimeInForce::Gtc,
    ///         },
    ///         cloid: Default::default(),
    ///     }],
    ///     grouping: OrderGrouping::Na,
    /// };
    ///
    /// let nonce = chrono::Utc::now().timestamp_millis() as u64;
    ///
    /// // Execute the multisig order
    /// let response = client
    ///     .multi_sig(&signer1, multisig_addr, signers, Action::Order(order), nonce)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn multi_sig<'a, S: SignerSync + Signer>(
        &'a self,
        lead: &'a S,
        multi_sig_user: Address,
        nonce: u64,
    ) -> MultiSig<'a, S> {
        MultiSig {
            lead,
            multi_sig_user,
            signers: VecDeque::new(),
            client: self,
            nonce,
        }
    }

    /// Send a signed action hashing.
    fn sign_and_send<S: SignerSync, A: Signable>(
        &self,
        signer: &S,
        action: A,
        nonce: u64,
        maybe_vault_address: Option<Address>,
        maybe_expires_after: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<ApiResponse>> + Send + 'static {
        let res = action.sign(
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
                .await?;
            Ok(res)
        }
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
pub struct MultiSig<'a, S: SignerSync + Signer> {
    lead: &'a S,
    multi_sig_user: Address,
    signers: VecDeque<&'a S>,
    nonce: u64,
    client: &'a Client,
}

impl<'a, S> MultiSig<'a, S>
where
    S: SignerSync + Signer,
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
    /// # Returns
    ///
    /// Returns `self` for method chaining.
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
    /// # Returns
    ///
    /// Returns `self` for method chaining.
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
    /// # Returns
    ///
    /// A future that resolves to a vector of `OrderResponseStatus` for each order in the batch,
    /// or an `ActionError` containing the failed order CLOIDs and error message.
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
    pub fn place(
        &self,
        batch: BatchOrder,
        vault_address: Option<Address>,
        expires_after: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<Vec<OrderResponseStatus>, ActionError<Cloid>>> + Send + 'static
    {
        let cloids: Vec<_> = batch.orders.iter().map(|req| req.cloid).collect();

        let res = multisig_collect_signatures(
            self.lead.address(),
            self.multi_sig_user,
            self.signers.iter().copied(),
            Action::Order(batch),
            self.nonce,
            self.client.chain,
        )
        .map(|action| {
            self.client
                .sign_and_send(&self.lead, action, self.nonce, vault_address, expires_after)
        });

        async move {
            let future = res.map_err(|err| ActionError {
                ids: cloids.clone(),
                err: err.to_string(),
            })?;
            let resp = future.await.map_err(|err| ActionError {
                ids: cloids.clone(),
                err: err.to_string(),
            })?;

            match resp {
                ApiResponse::Ok(OkResponse::Order { statuses }) => Ok(statuses),
                ApiResponse::Err(err) => Err(ActionError { ids: cloids, err }),
                _ => Err(ActionError {
                    ids: cloids,
                    err: format!("unexpected response type: {resp:?}"),
                }),
            }
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
    /// # Returns
    ///
    /// A future that resolves to `Ok(())` on success or an error on failure.
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
    pub fn send_usdc(&self, send: UsdSend) -> impl Future<Output = Result<()>> + Send + 'static {
        let nonce = send.time;
        let res = multisig_collect_signatures(
            self.lead.address(),
            self.multi_sig_user,
            self.signers.iter().copied(),
            send.into_action(self.client.chain().arbitrum_id(), self.client.chain())
                .into(),
            nonce,
            self.client.chain,
        )
        .map(|action| {
            self.client
                .sign_and_send(&self.lead, action, self.nonce, None, None)
        });

        async move {
            let future = res?;
            let resp = future.await?;

            match resp {
                ApiResponse::Ok(OkResponse::Default) => Ok(()),
                ApiResponse::Err(err) => anyhow::bail!("send_usdc: {err}"),
                _ => anyhow::bail!("send_usdc: unexpected response type: {resp:?}"),
            }
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
    /// # Returns
    ///
    /// A future that resolves to `Ok(())` on success or an error on failure.
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
    pub fn send_asset(&self, send: SendAsset) -> impl Future<Output = Result<()>> + Send + 'static {
        let nonce = send.nonce;
        let res = multisig_collect_signatures(
            self.lead.address(),
            self.multi_sig_user,
            self.signers.iter().copied(),
            send.into_action(self.client.chain().arbitrum_id(), self.client.chain())
                .into(),
            nonce,
            self.client.chain,
        )
        .map(|action| {
            self.client
                .sign_and_send(&self.lead, action, self.nonce, None, None)
        });

        async move {
            let future = res?;
            let resp = future.await?;

            match resp {
                ApiResponse::Ok(OkResponse::Default) => Ok(()),
                ApiResponse::Err(err) => anyhow::bail!("send_asset: {err}"),
                _ => anyhow::bail!("send_asset: unexpected response type: {resp:?}"),
            }
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
enum InfoRequest {
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
    AllMids,
}
