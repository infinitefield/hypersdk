//! WebSocket client for real-time HyperCore market data.
//!
//! This module provides a persistent WebSocket connection that automatically
//! reconnects on failure and manages subscriptions across reconnections.
//!
//! # Connection Status
//!
//! The connection yields [`Event`] which wraps connection state and data messages:
//!
//! - [`Event::Connected`] — Connection established (including after reconnection)
//! - [`Event::Disconnected`] — Connection lost (will auto-reconnect)
//! - [`Event::Message`] — Contains an [`Incoming`] data message
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
//! ws.subscribe(Subscription::L2Book {
//!     coin: "BTC".into(),
//!     n_sig_figs: None,
//!     mantissa: None,
//!     fast: None,
//! });
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
use futures::{SinkExt, StreamExt};
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    time::{interval, sleep, timeout},
};
use tokio_util::sync::CancellationToken;
use url::Url;
use yawc::{Frame, OpCode, Options, TcpWebSocket};

use crate::hypercore::types::{Incoming, Outgoing, Subscription};

struct Stream {
    stream: TcpWebSocket,
}

impl Stream {
    /// Establish a WebSocket connection.
    async fn connect(url: Url) -> Result<Self> {
        let stream = yawc::WebSocket::connect(url)
            .with_options(
                Options::default()
                    .with_no_delay()
                    .with_balanced_compression()
                    .with_utf8(),
            )
            .await?;

        Ok(Self { stream })
    }

    /// Subscribes to a topic.
    async fn subscribe(&mut self, subscription: Subscription) -> anyhow::Result<()> {
        let text = serde_json::to_string(&Outgoing::Subscribe { subscription })?;
        self.stream.send(Frame::text(text)).await?;
        Ok(())
    }

    /// Unsubscribes from a topic.
    async fn unsubscribe(&mut self, subscription: Subscription) -> anyhow::Result<()> {
        let text = serde_json::to_string(&Outgoing::Unsubscribe { subscription })?;
        self.stream.send(Frame::text(text)).await?;
        Ok(())
    }

    /// Send a ping
    async fn ping(&mut self) -> anyhow::Result<()> {
        let text = serde_json::to_string(&Outgoing::Ping)?;
        self.stream.send(Frame::text(text)).await?;
        Ok(())
    }

    /// Send a pong
    async fn pong(&mut self) -> anyhow::Result<()> {
        let text = serde_json::to_string(&Outgoing::Pong)?;
        self.stream.send(Frame::text(text)).await?;
        Ok(())
    }
}

impl futures::Stream for Stream {
    type Item = Incoming;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        while let Some(frame) = ready!(this.stream.poll_next_unpin(cx)) {
            if frame.opcode() == OpCode::Text {
                match serde_json::from_slice(frame.payload()) {
                    Ok(ok) => {
                        return Poll::Ready(Some(ok));
                    }
                    Err(err) => {
                        log::warn!("unable to parse: {}: {:?}", frame.as_str(), err);
                    }
                }
            } else {
                log::warn!(
                    "Hyperliquid sent a binary msg? {data:?}",
                    data = frame.payload()
                );
            }
        }

        Poll::Ready(None)
    }
}

type SubChannelData = (bool, Subscription);

/// Shared handle that keeps the WebSocket background task alive.
///
/// When all clones are dropped, the [`CancellationToken`] is cancelled and
/// the background reconnect loop exits gracefully.
#[derive(Clone)]
struct ConnectionGuard {
    /// Held solely to keep the token alive. When all guards drop, the token
    /// is cancelled and the background task exits.
    #[allow(dead_code)]
    token: CancellationToken,
}

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
/// # Graceful Shutdown
///
/// The background reconnect loop runs until all handles (`Connection`,
/// [`ConnectionHandle`], and [`ConnectionStream`]) are dropped. Once the last
/// handle is dropped, the background task exits cleanly. You can also call
/// [`close`](Self::close) to explicitly shut down the connection.
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
    guard: ConnectionGuard,
}

/// A handle for managing subscriptions to a WebSocket connection.
///
/// This handle is obtained by calling [`Connection::split()`] and allows for
/// subscribing and unsubscribing to channels independently of where the
/// event stream is being processed. It's useful for scenarios where you
/// want to manage subscriptions from a separate task or context.
///
/// The subscriptions managed by this handle persist across automatic
/// reconnections.
///
/// # Graceful Shutdown
///
/// The background task will shut down when **all** handles and streams are
/// dropped. To explicitly trigger shutdown, call [`close`](Self::close).
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore::{self, ws::Event, types::*};
/// use futures::StreamExt;
/// use tokio::spawn;
///
/// # async fn example() -> anyhow::Result<()> {
/// let ws = hypercore::mainnet_ws();
/// let (handle, mut stream) = ws.split();
///
/// // Manage subscriptions in a separate task
/// spawn(async move {
///     handle.subscribe(Subscription::Trades { coin: "BTC".into() });
///     handle.subscribe(Subscription::L2Book {
///         coin: "ETH".into(),
///         n_sig_figs: None,
///         mantissa: None,
///         fast: None,
///     });
///
///     // Later, unsubscribe
///     tokio::time::sleep(std::time::Duration::from_secs(60)).await;
///     handle.unsubscribe(Subscription::Trades { coin: "BTC".into() });
/// });
///
/// // Process events in the current task
/// while let Some(event) = stream.next().await {
///     match event {
///         Event::Message(Incoming::Trades(trades)) => {
///             println!("Received {} trades", trades.len());
///         }
///         _ => {}
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[allow(dead_code)]
#[derive(Clone)]
pub struct ConnectionHandle {
    tx: UnboundedSender<SubChannelData>,
    /// Keeps the CancellationToken alive; dropping this handle may trigger
    /// graceful shutdown of the background task if it was the last reference.
    #[allow(dead_code)]
    guard: ConnectionGuard,
}

/// A stream of events from a WebSocket connection.
///
/// This stream is obtained by calling [`Connection::split()`] and yields
/// [`Event`] items, which represent connection status changes or incoming
/// data messages.
///
/// It implements `futures::Stream`, allowing you to easily process events
/// using methods like `next().await` or `for_each()`.
///
/// # Graceful Shutdown
///
/// The background task will shut down when all handles and streams are dropped.
///
/// # Example
///
/// ```no_run
/// use hypersdk::hypercore::{self, ws::Event, types::*};
/// use futures::StreamExt;
///
/// # async fn example() -> anyhow::Result<()> {
/// let ws = hypercore::mainnet_ws();
/// let (_handle, mut stream) = ws.split();
///
/// while let Some(event) = stream.next().await {
///     match event {
///         Event::Connected => println!("Stream connected!"),
///         Event::Disconnected => println!("Stream disconnected"),
///         Event::Message(Incoming::Trades(trades)) => {
///             println!("Received {} trades", trades.len());
///         }
///         _ => {}
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[allow(dead_code)]
pub struct ConnectionStream {
    rx: UnboundedReceiver<Event>,
    /// Keeps the CancellationToken alive; dropping this stream may trigger
    /// graceful shutdown of the background task if it was the last reference.
    #[allow(dead_code)]
    guard: ConnectionGuard,
}

impl Connection {
    /// Creates a new WebSocket connection to the specified URL.
    ///
    /// The connection starts immediately and runs in the background,
    /// automatically reconnecting on failures. Connection status events
    /// ([`Event::Connected`], [`Event::Disconnected`]) will be emitted through
    /// the stream.
    ///
    /// The background task will exit gracefully when this `Connection` (or any
    /// handles derived from it via [`split`](Self::split)) is dropped.
    ///
    /// # Example
    ///
    /// Create a new WebSocket connection:
    /// `WebSocket::new(hypercore::mainnet_websocket_url())`
    pub fn new(url: Url) -> Self {
        let (tx, rx) = unbounded_channel();
        let (stx, srx) = unbounded_channel();
        let token = CancellationToken::new();
        tokio::spawn(connection(url, tx, srx, token.clone()));
        Self {
            rx,
            tx: stx,
            guard: ConnectionGuard { token },
        }
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
    /// - `ws.subscribe(Subscription::L2Book { coin: "ETH".into(), n_sig_figs: None, mantissa: None, fast: None })`
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

    /// Closes the WebSocket connection and shuts down the background task.
    ///
    /// After calling this, the connection will no longer receive messages
    /// and cannot be reused. The background reconnect loop will terminate.
    ///
    /// # Example
    ///
    /// Close the connection when done: `ws.close()`
    pub fn close(self) {
        drop(self);
    }

    /// Splits the connection into a subscription handle and an event stream.
    ///
    /// This is useful when you want to drive the stream in one task and
    /// manage subscriptions from another. Both returned halves participate
    /// in graceful shutdown — the background task exits when all handles
    /// and streams are dropped.
    pub fn split(self) -> (ConnectionHandle, ConnectionStream) {
        (
            ConnectionHandle {
                tx: self.tx,
                guard: self.guard.clone(),
            },
            ConnectionStream {
                rx: self.rx,
                guard: self.guard,
            },
        )
    }
}

impl futures::Stream for Connection {
    type Item = Event;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.rx.poll_recv(cx)
    }
}

impl ConnectionHandle {
    /// Subscribes to a WebSocket channel.
    ///
    /// The subscription will persist across reconnections. If you're already
    /// subscribed to this channel, this is a no-op.
    ///
    /// # Example
    ///
    /// Subscribe to market data:
    /// - `ws.subscribe(Subscription::Trades { coin: "BTC".into() })`
    /// - `ws.subscribe(Subscription::L2Book { coin: "ETH".into(), n_sig_figs: None, mantissa: None, fast: None })`
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

    /// Drops this handle, releasing its reference to the shared connection.
    ///
    /// The background task will shut down when **all** handles and streams
    /// are dropped. If other [`ConnectionHandle`] or [`ConnectionStream`]
    /// instances still exist, the connection remains active.
    ///
    /// # Example
    ///
    /// Close the connection when done: `drop(handle)`
    pub fn close(self) {
        drop(self);
    }
}

impl futures::Stream for ConnectionStream {
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
    shutdown: CancellationToken,
) {
    const MAX_MISSED_PONGS: u8 = 2;
    const MAX_RECONNECT_DELAY_MS: u64 = 5_000; // 5 seconds max
    const INITIAL_RECONNECT_DELAY_MS: u64 = 500;

    let mut subs: HashSet<Subscription> = HashSet::new();
    let mut reconnect_attempts = 0u32;

    loop {
        // Race the connect attempt (with timeout) against the shutdown signal.
        let mut stream = match tokio::select! {
            result = timeout(Duration::from_secs(10), Stream::connect(url.clone())) => {
                match result {
                    Ok(Ok(stream)) => Some(stream),
                    Ok(Err(err)) => {
                        log::error!("Unable to connect to {url}: {err:?}");
                        None
                    }
                    Err(_) => {
                        log::error!("Connection timeout to {url}");
                        None
                    }
                }
            }
            _ = shutdown.cancelled() => {
                break;
            }
        } {
            Some(stream) => stream,
            None => {
                // Exponential backoff: 500ms, 1s, 2s, 4s, 5s (capped)
                let delay_ms = (INITIAL_RECONNECT_DELAY_MS * (1u64 << reconnect_attempts))
                    .min(MAX_RECONNECT_DELAY_MS);
                reconnect_attempts = reconnect_attempts.saturating_add(1);

                log::debug!(
                    "Reconnecting in {}ms (attempt {})",
                    delay_ms,
                    reconnect_attempts
                );

                // Sleep but respect shutdown signal
                if tokio::select! {
                    _ = sleep(Duration::from_millis(delay_ms)) => false,
                    _ = shutdown.cancelled() => true,
                } {
                    break;
                }

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
                _ = shutdown.cancelled() => {
                    // Shutdown signal received — exit gracefully
                    log::debug!("Shutdown signal received, closing WebSocket connection");
                    break;
                }
            }
        }

        log::info!("Disconnected from {url}, attempting to reconnect...");
        let _ = tx.send(Event::Disconnected);
    }

    log::debug!("WebSocket background task shutting down");
}
