use alloy::primitives::B128;
use alloy::signers::local::PrivateKeySigner;
use clap::Args;
use futures::StreamExt;
use hypersdk::hypercore::{
    self, BatchCancel, BatchModify, BatchOrder, Cancel, Chain, HttpClient, Modify, OidOrCloid,
    OrderGrouping, OrderRequest, OrderTypePlacement, PriceTick, TimeInForce,
    types::{Incoming, OrderResponseStatus, Side as BookSide, Subscription},
    ws::Event,
};
use rust_decimal::{Decimal, RoundingStrategy};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::{Instant, Interval, interval};

use crate::SignerArgs;
use crate::orders::Side;
use crate::utils::{find_signer_sync, resolve_asset_for_subscription, resolve_market};

#[derive(Args, derive_more::Deref)]
pub struct TwapCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Asset name (e.g., "BTC", "PURR/USDC", "xyz:BTC")
    #[arg(long)]
    pub asset: String,

    /// Order side (buy or sell)
    #[arg(long)]
    pub side: Side,

    /// Total size to execute
    #[arg(long)]
    pub size: Decimal,

    /// Number of individual orders to split into
    #[arg(long, default_value = "10")]
    pub slices: u32,

    /// Total duration of the TWAP in seconds (e.g., 60 = 1 minute, 300 = 5 minutes)
    #[arg(long)]
    pub duration: u64,

    /// Slippage tolerance as a percentage (e.g., 0.5 = 0.5%).
    /// For buys: limit_price = best_ask * (1 + pct/100).
    /// For sells: limit_price = best_bid * (1 - pct/100).
    #[arg(long, default_value = "0.5")]
    pub slippage_pct: Decimal,

    /// Reduce-only order
    #[arg(long, default_value = "false")]
    pub reduce_only: bool,

    /// Size randomization factor (0 to 100, as a percentage).
    /// Each slice varies by +/- this percent of the base slice size while
    /// keeping the total exact. E.g., 30 means each slice can be 70%-130% of average.
    #[arg(long, default_value = "30")]
    pub randomize: Decimal,

    /// Timing jitter factor (0 to 100, as a percentage).
    /// Each interval varies by +/- this percent. E.g., 20 means +/- 20% of interval.
    #[arg(long, default_value = "20")]
    pub jitter: Decimal,

    /// Quote mode: place ALO (maker-only) orders at top of book instead of IOC.
    /// Chases the best bid/ask on every BBO update, canceling and replacing.
    #[arg(long, default_value = "false")]
    pub quote: bool,
}

struct TwapState {
    slice_sizes: Vec<Decimal>,
    current_slice: usize,
    filled_total: Decimal,
    total_size: Decimal,
    best_bid: Option<Decimal>,
    best_ask: Option<Decimal>,
    has_book: bool,
    tick: PriceTick,
    resting_oid: Option<u64>,
    slice_remaining: Decimal,
    last_quote_px: Option<Decimal>,
    last_request: Instant,
}

const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(60);

impl TwapState {
    fn new(slice_sizes: Vec<Decimal>, total_size: Decimal, tick: PriceTick) -> Self {
        let first_size = slice_sizes.first().copied().unwrap_or(Decimal::ZERO);
        Self {
            slice_sizes,
            current_slice: 0,
            filled_total: Decimal::ZERO,
            total_size,
            best_bid: None,
            best_ask: None,
            has_book: false,
            tick,
            resting_oid: None,
            slice_remaining: first_size,
            last_quote_px: None,
            last_request: Instant::now(),
        }
    }

    fn finished(&self) -> bool {
        self.current_slice >= self.slice_sizes.len()
    }

    fn current_size(&self) -> Decimal {
        self.slice_sizes[self.current_slice]
    }

    fn update_bbo(&mut self, incoming: &Incoming) -> bool {
        if let Incoming::Bbo(bbo) = incoming {
            if let Some(b) = bbo.bid() {
                self.best_bid = Some(b.px);
            }
            if let Some(a) = bbo.ask() {
                self.best_ask = Some(a.px);
            }
            self.has_book = self.best_bid.is_some() && self.best_ask.is_some();
            return true;
        }
        false
    }

    fn limit_price(&self, side: Side, slippage_mult: Decimal) -> Option<Decimal> {
        let raw = match side {
            Side::Buy => self.best_ask.map(|a| a * (Decimal::ONE + slippage_mult)),
            Side::Sell => self.best_bid.map(|b| b * (Decimal::ONE - slippage_mult)),
        }?;
        let book_side = match side {
            Side::Buy => BookSide::Bid,
            Side::Sell => BookSide::Ask,
        };
        self.tick.round_by_side(book_side, raw, false)
    }

    fn top_of_book_price(&self, side: Side) -> Option<Decimal> {
        let raw = match side {
            Side::Buy => self.best_bid?,
            Side::Sell => self.best_ask?,
        };
        let book_side = match side {
            Side::Buy => BookSide::Bid,
            Side::Sell => BookSide::Ask,
        };
        self.tick.round_by_side(book_side, raw, false)
    }

    fn advance_slice(&mut self) {
        self.current_slice += 1;
        if !self.finished() {
            self.slice_remaining = self.current_size();
        }
        self.resting_oid = None;
        self.last_quote_px = None;
    }

    async fn throttle(&mut self) {
        let elapsed = self.last_request.elapsed();
        if elapsed < MIN_REQUEST_INTERVAL {
            tokio::time::sleep(MIN_REQUEST_INTERVAL - elapsed).await;
        }
        self.last_request = Instant::now();
    }

    fn progress_pct(&self) -> Decimal {
        if self.total_size.is_zero() {
            return Decimal::ZERO;
        }
        (self.filled_total * Decimal::ONE_HUNDRED) / self.total_size
    }

    fn book_str(&self) -> String {
        format!(
            "{} / {}",
            self.best_bid.unwrap_or(Decimal::ZERO),
            self.best_ask.unwrap_or(Decimal::ZERO),
        )
    }
}

impl TwapCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        anyhow::ensure!(self.slices >= 2, "slices must be at least 2");
        anyhow::ensure!(self.duration > 0, "duration must be positive");
        anyhow::ensure!(
            self.slippage_pct >= Decimal::ZERO,
            "slippage_pct must be non-negative"
        );
        anyhow::ensure!(
            self.randomize >= Decimal::ZERO && self.randomize <= Decimal::ONE_HUNDRED,
            "randomize must be between 0 and 100"
        );
        anyhow::ensure!(
            self.jitter >= Decimal::ZERO && self.jitter <= Decimal::ONE_HUNDRED,
            "jitter must be between 0 and 100"
        );

        let interval_secs = self.duration / self.slices as u64;
        anyhow::ensure!(
            interval_secs >= 1,
            "duration too short for {} slices (need at least {} seconds)",
            self.slices,
            self.slices
        );

        let client = HttpClient::new(self.chain);
        let signer = find_signer_sync(&self.signer)?;
        let market = resolve_market(&client, &self.asset).await?;
        let resolved = resolve_asset_for_subscription(&client, &self.asset).await?;

        let core = match self.chain {
            Chain::Mainnet => hypercore::mainnet(),
            Chain::Testnet => hypercore::testnet(),
        };

        let mut ws = core.websocket();
        ws.subscribe(Subscription::Bbo {
            coin: resolved.coin.clone(),
        });
        if self.quote {
            ws.subscribe(Subscription::OrderUpdates {
                user: signer.address(),
            });
        }

        let randomize_frac = self.randomize / Decimal::ONE_HUNDRED;
        let jitter_frac = self.jitter / Decimal::ONE_HUNDRED;
        let slippage_mult = self.slippage_pct / Decimal::ONE_HUNDRED;

        let slice_sizes =
            randomize_sizes(self.size, self.slices, randomize_frac, market.sz_decimals)?;

        let mut state = TwapState::new(slice_sizes, self.size, market.tick);

        if self.quote {
            self.run_quote(&client, &signer, market.index, &mut state, &mut ws)
                .await
        } else {
            self.run_ioc(
                &client,
                &signer,
                market.index,
                &mut state,
                &mut ws,
                interval_secs,
                jitter_frac,
                slippage_mult,
            )
            .await
        }
    }

    async fn run_ioc(
        &self,
        client: &HttpClient,
        signer: &PrivateKeySigner,
        asset: usize,
        state: &mut TwapState,
        ws: &mut (impl futures::Stream<Item = Event> + Unpin),
        interval_secs: u64,
        jitter_frac: Decimal,
        slippage_mult: Decimal,
    ) -> anyhow::Result<()> {
        let mut ticker = new_ticker(interval_secs, jitter_frac);
        ticker.tick().await;

        eprintln!("Connecting to {} BBO feed...", self.asset);

        loop {
            tokio::select! {
                event = ws.next() => {
                    match event {
                        Some(Event::Message(msg)) => { state.update_bbo(&msg); }
                        Some(Event::Connected) => eprintln!("Connected to websocket"),
                        Some(Event::Disconnected) => eprintln!("Disconnected, reconnecting..."),
                        None => anyhow::bail!("websocket closed"),
                    }
                }
                _ = ticker.tick() => {
                    if !state.has_book {
                        continue;
                    }

                    if state.current_slice == 0 {
                        println!(
                            "Starting stealth TWAP: {} {} {} in {} slices over {} (~{}s between)",
                            self.side, self.size, self.asset,
                            self.slices, format_duration(Duration::from_secs(self.duration)),
                            interval_secs,
                        );
                        println!("Signer: {}", signer.address());
                        println!("Slippage: {}% | Book: {}", self.slippage_pct, state.book_str());
                        println!();
                    }

                    let limit_px = match state.limit_price(self.side, slippage_mult) {
                        Some(px) => px,
                        None => {
                            eprintln!("No price available, skipping tick");
                            continue;
                        }
                    };

                    let sz = state.current_size();

                    println!(
                        "[{}/{}] {} {} {} @ limit {} (book: {})",
                        state.current_slice + 1, self.slices,
                        self.side, sz, self.asset, limit_px, state.book_str(),
                    );

                    let order = self.make_order(asset, limit_px, sz, TimeInForce::Ioc);
                    let batch = BatchOrder { orders: vec![order], grouping: OrderGrouping::Na, builder: None };

                    match client.place(signer, batch, nonce(), None, None).await {
                        Ok(statuses) => {
                            for status in &statuses {
                                println!("  {:?}", status);
                            }
                            state.filled_total += sz;
                            state.current_slice += 1;
                        }
                        Err(err) => {
                            println!("  FAILED: {}", err.message());
                            break;
                        }
                    }

                    println!(
                        "  Progress: {}/{} ({}%)",
                        state.filled_total, self.size,
                        state.progress_pct().round_dp(1),
                    );

                    if state.finished() {
                        break;
                    }

                    ticker = new_ticker(interval_secs, jitter_frac);
                    ticker.tick().await;
                }
            }
        }

        println!();
        println!("TWAP complete. Total submitted: {}", state.filled_total);
        Ok(())
    }

    async fn run_quote(
        &self,
        client: &HttpClient,
        signer: &PrivateKeySigner,
        asset: usize,
        state: &mut TwapState,
        ws: &mut (impl futures::Stream<Item = Event> + Unpin),
    ) -> anyhow::Result<()> {
        let interval_secs = self.duration / self.slices as u64;
        let jitter_frac = self.jitter / Decimal::ONE_HUNDRED;
        let mut ticker = new_ticker(interval_secs, jitter_frac);
        ticker.tick().await;

        let mut slice_started = false;

        eprintln!("Connecting to {} BBO + OrderUpdates feed...", self.asset);

        loop {
            tokio::select! {
                event = ws.next() => {
                    let msg = match event {
                        Some(Event::Message(msg)) => msg,
                        Some(Event::Connected) => { eprintln!("Connected to websocket"); continue; }
                        Some(Event::Disconnected) => { eprintln!("Disconnected, reconnecting..."); continue; }
                        None => anyhow::bail!("websocket closed"),
                    };

                    if state.update_bbo(&msg) && state.has_book && slice_started {
                        if let Some(new_px) = state.top_of_book_price(self.side) {
                            if state.last_quote_px != Some(new_px) {
                                state.throttle().await;
                                if let Some(oid) = state.resting_oid {
                                    self.modify_quote(client, signer, asset, state, oid, new_px).await?;
                                } else if state.slice_remaining > Decimal::ZERO {
                                    self.place_quote(client, signer, asset, state, new_px).await?;
                                }
                            }
                        }
                    }

                    if let Incoming::OrderUpdates(updates) = &msg {
                        for update in updates {
                            let Some(resting_oid) = state.resting_oid else {
                                continue
                            };
                            if update.order.oid != resting_oid {
                                continue;
                            }

                            let filled = update.order.orig_sz - update.order.sz;
                            let prev_filled = state.current_size() - state.slice_remaining;
                            let new_fill = filled - prev_filled;
                            if new_fill > Decimal::ZERO {
                                state.filled_total += new_fill;
                                state.slice_remaining = state.current_size() - filled;
                                println!(
                                    "  Fill: +{} (remaining: {}, total: {}/{})",
                                    new_fill, state.slice_remaining,
                                    state.filled_total, state.total_size,
                                );
                            }

                            if update.status.is_finished() {
                                state.resting_oid = None;
                                if state.slice_remaining <= Decimal::ZERO {
                                    println!(
                                        "  Slice {}/{} complete ({}%)",
                                        state.current_slice + 1, self.slices,
                                        state.progress_pct().round_dp(1),
                                    );
                                    state.advance_slice();
                                    slice_started = false;
                                }
                            }
                        }
                        if state.finished() { break; }
                    }
                }
                _ = ticker.tick() => {
                    if !state.has_book || state.finished() || slice_started {
                        continue;
                    }

                    if state.current_slice == 0 {
                        println!(
                            "Starting quote TWAP: {} {} {} in {} slices over {} (~{}s between)",
                            self.side, self.size, self.asset,
                            self.slices, format_duration(Duration::from_secs(self.duration)),
                            interval_secs,
                        );
                        println!("Signer: {}", signer.address());
                        println!("Mode: ALO (maker-only) | Book: {}", state.book_str());
                        println!();
                    }

                    slice_started = true;
                    let Some(px) = state.top_of_book_price(self.side) else { continue };

                    println!(
                        "[{}/{}] Quoting {} {} @ {} (book: {})",
                        state.current_slice + 1, self.slices,
                        self.side, state.slice_remaining, self.asset, state.book_str(),
                    );

                    state.throttle().await;
                    self.place_quote(client, signer, asset, state, px).await?;

                    ticker = new_ticker(interval_secs, jitter_frac);
                    ticker.tick().await;
                }
            }
        }

        if let Some(oid) = state.resting_oid.take() {
            let cancel = BatchCancel {
                cancels: vec![Cancel { asset, oid }],
                fast: false,
            };
            let _ = client.cancel(signer, cancel, nonce(), None, None).await;
        }

        println!();
        println!("Quote TWAP complete. Total filled: {}", state.filled_total);
        Ok(())
    }

    fn make_order(
        &self,
        asset: usize,
        limit_px: Decimal,
        sz: Decimal,
        tif: TimeInForce,
    ) -> OrderRequest {
        OrderRequest {
            asset,
            is_buy: self.side.is_buy(),
            limit_px,
            sz,
            reduce_only: self.reduce_only,
            order_type: OrderTypePlacement::Limit { tif },
            cloid: B128::random(),
        }
    }

    async fn place_quote(
        &self,
        client: &HttpClient,
        signer: &PrivateKeySigner,
        asset: usize,
        state: &mut TwapState,
        px: Decimal,
    ) -> anyhow::Result<()> {
        let order = self.make_order(asset, px, state.slice_remaining, TimeInForce::Alo);
        let batch = BatchOrder {
            orders: vec![order],
            grouping: OrderGrouping::Na,
            builder: None,
        };

        match client.place(signer, batch, nonce(), None, None).await {
            Ok(statuses) => {
                for status in &statuses {
                    if let OrderResponseStatus::Resting { oid, .. } = status {
                        state.resting_oid = Some(*oid);
                        state.last_quote_px = Some(px);
                        eprintln!("  Resting @ {} (oid: {})", px, oid);
                    } else {
                        eprintln!("  {:?}", status);
                    }
                }
            }
            Err(err) => eprintln!("  Place failed: {}", err.message()),
        }
        Ok(())
    }

    async fn modify_quote(
        &self,
        client: &HttpClient,
        signer: &PrivateKeySigner,
        asset: usize,
        state: &mut TwapState,
        oid: u64,
        px: Decimal,
    ) -> anyhow::Result<()> {
        let order = self.make_order(asset, px, state.slice_remaining, TimeInForce::Alo);
        let batch = BatchModify {
            modifies: vec![Modify {
                oid: OidOrCloid::Left(oid),
                order,
            }],
        };

        match client.modify(signer, batch, nonce(), None, None).await {
            Ok(statuses) => {
                for status in &statuses {
                    if let OrderResponseStatus::Resting { oid, .. } = status {
                        state.resting_oid = Some(*oid);
                        state.last_quote_px = Some(px);
                        eprintln!("  Modified -> {} (oid: {})", px, oid);
                    } else {
                        state.resting_oid = None;
                        eprintln!("  Modify: {:?}", status);
                    }
                }
            }
            Err(err) => {
                state.resting_oid = None;
                eprintln!("  Modify failed: {}", err.message());
            }
        }
        Ok(())
    }
}

fn nonce() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn new_ticker(base_secs: u64, jitter_frac: Decimal) -> Interval {
    interval(jittered_duration(base_secs, jitter_frac))
}

fn jittered_duration(base_secs: u64, jitter_frac: Decimal) -> Duration {
    if jitter_frac.is_zero() {
        return Duration::from_secs(base_secs);
    }
    let r = random_decimal();
    let multiplier = Decimal::ONE + jitter_frac * (Decimal::TWO * r - Decimal::ONE);
    let millis = Decimal::from(base_secs) * Decimal::from(1000u32) * multiplier;
    let millis_u64: u64 = millis.try_into().unwrap_or(base_secs * 1000);
    Duration::from_millis(millis_u64)
}

fn random_decimal() -> Decimal {
    Decimal::from(rand_09::random_range(0u32..=1_000_000)) / Decimal::from(1_000_000u32)
}

fn randomize_sizes(
    total: Decimal,
    slices: u32,
    factor: Decimal,
    sz_decimals: u32,
) -> anyhow::Result<Vec<Decimal>> {
    let trunc =
        |d: Decimal| -> Decimal { d.round_dp_with_strategy(sz_decimals, RoundingStrategy::ToZero) };

    if factor.is_zero() || slices <= 1 {
        let base = trunc(total / Decimal::from(slices));
        let mut sizes = vec![base; slices as usize];
        let sum: Decimal = sizes.iter().copied().sum();
        if let Some(last) = sizes.last_mut() {
            *last += total - sum;
        }
        return Ok(sizes);
    }

    let avg = total / Decimal::from(slices);
    let mut sizes = Vec::with_capacity(slices as usize);
    for _ in 0..slices {
        let r = random_decimal();
        let jitter = Decimal::ONE + factor * (Decimal::TWO * r - Decimal::ONE);
        sizes.push(trunc(avg * jitter));
    }

    // Normalize so they sum to total
    let sum: Decimal = sizes.iter().copied().sum();
    if !sum.is_zero() {
        let scale = total / sum;
        for size in &mut sizes {
            *size = trunc(*size * scale);
        }
    }

    // Fix rounding remainder into last slice
    let sum: Decimal = sizes.iter().copied().sum();
    if let Some(last) = sizes.last_mut() {
        *last += total - sum;
    }

    for size in &sizes {
        anyhow::ensure!(
            *size > Decimal::ZERO,
            "randomization produced a non-positive slice; reduce --randomize or increase --size"
        );
    }

    Ok(sizes)
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}
