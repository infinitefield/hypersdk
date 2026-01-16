//! WebSocket client for real-time HyperCore market data.
//!
//! This module provides a persistent WebSocket connection that automatically
//! reconnects on failure and manages subscriptions across reconnections.
//!
//! # Connection Status
//!
//! The connection yields [`Event`] which wraps connection state and data messages:
//!
//! - [`Event::Connected`] - Connection established (including after reconnection)
//! - [`Event::Disconnected`] - Connection lost (will auto-reconnect)
//! - [`Event::Message`] - Contains an [`Incoming`] data message
//!
//! You can also check the current connection status using [`Connection::is_connected()`].
//!
//! # Examples
//!
//! ## Handle Connection Status
//!
//! ```no_run
//! use hypersdk::hypercore::{self, ws::Event, types::*};
//! use futures::StreamExt;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let mut ws = hypercore::mainnet_ws();
//! ws.subscribe(Subscription::Trades { coin: "BTC".into() });
//!
//! while let Some(event) = ws.next().await {
//!     match event {
//!         Event::Connected => {
//!             println!("Connected to WebSocket");
//!         }
//!         Event::Disconnected => {
//!             println!("Disconnected");
//!         }
//!         Event::Message(msg) => match msg {
//!             Incoming::Trades(trades) => {
//!                 for trade in trades {
//!                     println!("Trade: {} {} @ {}", trade.side, trade.sz, trade.px);
//!                 }
//!             }
//!             _ => {}
//!         }
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Subscribe to Market Data
//!
//! ```no_run
//! use hypersdk::hypercore::{self, ws::Event, types::*};
//! use futures::StreamExt;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let mut ws = hypercore::mainnet_ws();
//!
//! // Subscribe to trades and orderbook
//! ws.subscribe(Subscription::Trades { coin: "BTC".into() });
//! ws.subscribe(Subscription::L2Book { coin: "BTC".into() });
//!
//! while let Some(event) = ws.next().await {
//!     let Event::Message(msg) = event else { continue };
//!     match msg {
//!         Incoming::Trades(trades) => {
//!             for trade in trades {
//!                 println!("Trade: {} {} @ {}", trade.side, trade.sz, trade.px);
//!             }
//!         }
//!         Incoming::L2Book(book) => {
//!             println!("Book update: {} levels", book.levels[0].len() + book.levels[1].len());
//!         }
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Subscribe to User Events
//!
//! ```no_run
//! use hypersdk::hypercore::{self, ws::Event, types::*};
//! use hypersdk::Address;
//! use futures::StreamExt;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let mut ws = hypercore::mainnet_ws();
//! let user: Address = "0x...".parse()?;
//!
//! // Subscribe to order updates and fills
//! ws.subscribe(Subscription::OrderUpdates { user });
//! ws.subscribe(Subscription::UserFills { user });
//!
//! while let Some(event) = ws.next().await {
//!     let Event::Message(msg) = event else { continue };
//!     match msg {
//!         Incoming::OrderUpdates(updates) => {
//!             for update in updates {
//!                 println!("Order {}: {:?}", update.order.oid, update.status);
//!             }
//!         }
//!         Incoming::UserFills { fills, .. } => {
//!             for fill in fills {
//!                 println!("Fill: {} @ {}", fill.sz, fill.px);
//!             }
//!         }
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use std::{
    collections::HashSet,
    pin::Pin,
    task::{Context, Poll, ready},
    time::Duration,
};

use anyhow::Result;
use futures::StreamExt;
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    time::{interval, sleep, timeout},
};
use url::Url;
use yawc::{Options, WebSocket};

use crate::hypercore::types::{Incoming, Outgoing, Subscription};

struct Stream {
    stream: WebSocket,
}

impl Stream {
    /// Establish a WebSocket connection.
    async fn connect(url: Url) -> Result<Self> {
        let stream = yawc::WebSocket::connect(url)
            .with_options(Options::default().with_no_delay())
            .await?;

        Ok(Self { stream })
    }

    /// Subscribes to a topic.
    async fn subscribe(&mut self, subscription: Subscription) -> anyhow::Result<()> {
        self.stream
            .send_json(&Outgoing::Subscribe { subscription })
            .await?;
        Ok(())
    }

    /// Unsubscribes from a topic.
    async fn unsubscribe(&mut self, subscription: Subscription) -> anyhow::Result<()> {
        self.stream
            .send_json(&Outgoing::Unsubscribe { subscription })
            .await?;
        Ok(())
    }

    /// Send a ping
    async fn ping(&mut self) -> anyhow::Result<()> {
        self.stream.send_json(&Outgoing::Ping).await?;
        Ok(())
    }

    /// Send a pong
    async fn pong(&mut self) -> anyhow::Result<()> {
        self.stream.send_json(&Outgoing::Pong).await?;
        Ok(())
    }
}

impl futures::Stream for Stream {
    type Item = Incoming;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        while let Some(item) = ready!(this.stream.poll_next_unpin(cx)) {
            match serde_json::from_slice(&item.payload) {
                Ok(ok) => {
                    return Poll::Ready(Some(ok));
                }
                Err(err) => {
                    if let Ok(s) = std::str::from_utf8(&item.payload) {
                        log::warn!("unable to parse: {}: {:?}", s, err);
                    }
                }
            }
        }

        Poll::Ready(None)
    }
}

type SubChannelData = (bool, Subscription);

/// WebSocket event representing either a connection state change or a data message.
///
/// This enum cleanly separates connection lifecycle events from actual data messages,
/// allowing you to handle each appropriately.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore::{self, ws::Event, types::*};
/// use futures::StreamExt;
///
/// # async fn example() {
/// let mut ws = hypercore::mainnet_ws();
/// ws.subscribe(Subscription::Trades { coin: "BTC".into() });
///
/// while let Some(event) = ws.next().await {
///     match event {
///         Event::Connected => println!("Connected!"),
///         Event::Disconnected => println!("Disconnected"),
///         Event::Message(msg) => {
///             // Handle data messages
///         }
///     }
/// }
/// # }
/// ```
#[derive(Clone, Debug)]
pub enum Event {
    /// WebSocket connection established.
    ///
    /// Sent when a connection is successfully established, including after reconnection.
    /// Subscriptions are automatically restored after reconnection.
    Connected,
    /// WebSocket connection lost.
    ///
    /// Sent when the connection is unexpectedly closed. The connection will
    /// automatically attempt to reconnect.
    Disconnected,
    /// A data message received from the WebSocket.
    Message(Incoming),
}

/// Persistent WebSocket connection with automatic reconnection.
///
/// This connection automatically handles:
/// - Reconnection on connection failure
/// - Re-subscription after reconnection
/// - Periodic ping/pong to keep the connection alive
/// - Connection status notifications via [`Event`]
///
/// The connection implements `futures::Stream`, yielding [`Event`] items that
/// wrap both connection state changes and data messages.
///
/// # Connection Status
///
/// The connection emits status events through the stream:
/// - [`Event::Connected`] - Connection established (including after reconnection)
/// - [`Event::Disconnected`] - Connection lost
/// - [`Event::Message`] - Contains an [`Incoming`] data message
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore::{self, ws::Event, types::*};
/// use futures::StreamExt;
///
/// # async fn example() {
/// let mut ws = hypercore::mainnet_ws();
/// ws.subscribe(Subscription::Trades { coin: "BTC".into() });
///
/// while let Some(event) = ws.next().await {
///     match event {
///         Event::Connected => {
///             println!("Connected!");
///         }
///         Event::Disconnected => {
///             println!("Disconnected");
///         }
///         Event::Message(Incoming::Trades(trades)) => {
///             // Handle trades...
///         }
///         _ => {}
///     }
/// }
/// # }
/// ```
pub struct Connection {
    rx: UnboundedReceiver<Event>,
    tx: UnboundedSender<SubChannelData>,
}

impl Connection {
    /// Creates a new WebSocket connection to the specified URL.
    ///
    /// The connection starts immediately and runs in the background,
    /// automatically reconnecting on failures. Connection status events
    /// ([`Event::Connected`], [`Event::Disconnected`]) will be emitted through
    /// the stream.
    ///
    /// # Example
    ///
    /// Create a new WebSocket connection:
    /// `WebSocket::new(hypercore::mainnet_websocket_url())`
    pub fn new(url: Url) -> Self {
        let (tx, rx) = unbounded_channel();
        let (stx, srx) = unbounded_channel();
        tokio::spawn(connection(url, tx, srx));
        Self { rx, tx: stx }
    }

    /// Subscribes to a WebSocket channel.
    ///
    /// The subscription will persist across reconnections. If you're already
    /// subscribed to this channel, this is a no-op.
    ///
    /// # Example
    ///
    /// Subscribe to market data:
    /// - `ws.subscribe(Subscription::Trades { coin: "BTC".into() })`
    /// - `ws.subscribe(Subscription::L2Book { coin: "ETH".into() })`
    pub fn subscribe(&self, subscription: Subscription) {
        let _ = self.tx.send((true, subscription));
    }

    /// Unsubscribes from a WebSocket channel.
    ///
    /// Stops receiving updates for this subscription. Does nothing if you're
    /// not currently subscribed to this channel.
    ///
    /// # Example
    ///
    /// Unsubscribe from a channel:
    /// `ws.unsubscribe(Subscription::Trades { coin: "BTC".into() })`
    pub fn unsubscribe(&self, subscription: Subscription) {
        let _ = self.tx.send((false, subscription));
    }

    /// Closes the WebSocket connection.
    ///
    /// After calling this, the connection will no longer receive messages
    /// and cannot be reused.
    ///
    /// # Example
    ///
    /// Close the connection when done: `ws.close()`
    pub fn close(self) {
        drop(self);
    }
}

impl futures::Stream for Connection {
    type Item = Event;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.rx.poll_recv(cx)
    }
}

async fn connection(
    url: Url,
    tx: UnboundedSender<Event>,
    mut srx: UnboundedReceiver<SubChannelData>,
) {
    const MAX_MISSED_PONGS: u8 = 2;
    const MAX_RECONNECT_DELAY_MS: u64 = 5_000; // 5 seconds max
    const INITIAL_RECONNECT_DELAY_MS: u64 = 500;

    let mut subs: HashSet<Subscription> = HashSet::new();
    let mut reconnect_attempts = 0u32;

    loop {
        let mut stream = match timeout(Duration::from_secs(10), Stream::connect(url.clone())).await
        {
            Ok(ok) => match ok {
                Ok(ok) => ok,
                Err(err) => {
                    log::error!("Unable to connect to {url}: {err:?}");

                    // Exponential backoff: 500ms, 1s, 2s, 4s, 5s (capped)
                    let delay_ms = (INITIAL_RECONNECT_DELAY_MS * (1u64 << reconnect_attempts))
                        .min(MAX_RECONNECT_DELAY_MS);
                    reconnect_attempts = reconnect_attempts.saturating_add(1);

                    log::debug!(
                        "Reconnecting in {}ms (attempt {})",
                        delay_ms,
                        reconnect_attempts
                    );
                    sleep(Duration::from_millis(delay_ms)).await;

                    continue;
                }
            },
            Err(err) => {
                log::error!("Connection timeout to {url}: {err:?}");

                let delay_ms = (INITIAL_RECONNECT_DELAY_MS * (1u64 << reconnect_attempts))
                    .min(MAX_RECONNECT_DELAY_MS);
                reconnect_attempts = reconnect_attempts.saturating_add(1);

                log::debug!(
                    "Reconnecting in {}ms (attempt {})",
                    delay_ms,
                    reconnect_attempts
                );
                sleep(Duration::from_millis(delay_ms)).await;

                continue;
            }
        };

        log::debug!("Connected to {url}");
        reconnect_attempts = 0; // Reset on successful connection
        let _ = tx.send(Event::Connected);

        // Re-subscribe to all active subscriptions after reconnection
        if !subs.is_empty() {
            log::debug!("Re-subscribing to {} channels", subs.len());
            for sub in subs.iter() {
                log::debug!("Re-subscribing to {sub}");
                if let Err(err) = stream.subscribe(sub.clone()).await {
                    log::error!("Failed to re-subscribe to {sub}: {err:?}");
                }
            }
        }

        let mut ping_interval = interval(Duration::from_secs(5));
        let mut missed_pongs: u8 = 0;

        loop {
            tokio::select! {
                _ = ping_interval.tick() => {
                    if missed_pongs >= MAX_MISSED_PONGS {
                        log::warn!("Missed {missed_pongs} pongs, reconnecting...");
                        break;
                    }

                    if stream.ping().await.is_ok() {
                        missed_pongs += 1;
                    }
                }
                maybe_item = stream.next() => {
                    let Some(item) = maybe_item else { break; };
                    match item {
                        Incoming::Pong => {
                            missed_pongs = 0;
                        }
                        Incoming::Ping => {
                            let _ = stream.pong().await;
                        }
                        _ => {
                            let _ = tx.send(Event::Message(item));
                        }
                    }
                }
                item = srx.recv() => {
                    let Some((is_sub, sub)) = item else { return };
                    if is_sub {
                        if !subs.insert(sub.clone()) {
                            log::debug!("Already subscribed to {sub:?}");
                            continue;
                        }

                        if let Err(err) = stream.subscribe(sub).await {
                            log::error!("Subscribing: {err:?}");
                            break;
                        }
                    } else if subs.remove(&sub) {
                        if let Err(err) = stream.unsubscribe(sub).await {
                            log::error!("Unsubscribing: {err:?}");
                            break;
                        }
                    }
                }
            }
        }

        log::warn!("Disconnected from {url}, attempting to reconnect...");
        let _ = tx.send(Event::Disconnected);
    }
}
