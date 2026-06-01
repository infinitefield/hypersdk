//! Hyperliquid gossip priority Dutch auction.
//!
//! Hyperliquid's gossip network uses **5 slots** (indices 0–4) for read-priority
//! ordering. Winning a slot makes your node receive tx data ~10ms faster per level
//! than non-winners. All slots reset on a synchronized cycle (~3 minutes).
//!
//! ## How the auction works
//!
//! | `currentGas`        | Meaning                                         |
//! |---------------------|-------------------------------------------------|
//! | not null            | Auction RUNNING at displayed price. Bid now.    |
//! | null                | Settled — winner set, cannot bid this cycle.    |
//!
//! You pay the **live `currentGas` price** at TX mining time — not your `--max`.
//! Any difference between `--max` and the actual cost is refunded automatically.
//!
//! ## Usage
//!
//! ```bash
//! hypecli prio status          # Check current prices and state
//! hypecli prio bid --max X     # Bid on a slot
//! ```
//!
//! <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/priority-fees>

use clap::{Args, Subcommand};
use hypersdk::hypercore::types::{OkResponse, Response};
use hypersdk::hypercore::{Chain, HttpClient, NonceHandler};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;

use crate::SignerArgs;
use crate::utils::find_signer_sync;

#[derive(Subcommand)]
pub enum PrioCmd {
    /// Query current Dutch auction prices for all 5 slots.
    ///
    /// `currentGas != null` → auction RUNNING at that price.
    /// `currentGas == null` → settled, cannot bid this cycle.
    ///
    /// Prints `started <timestamp>` so you know when the cycle began.
    Status(StatusCmd),

    /// Place a signed bid on a gossip priority slot.
    ///
    /// You pay the **live `currentGas` price** at TX mining time — not your `--max`.
    /// Your `--max` is only a ceiling; any difference is refunded automatically.
    ///
    /// ## Auction states
    ///
    /// | State              | Meaning                                      |
    /// |--------------------|----------------------------------------------|
    /// | `currentGas != null` | Auction RUNNING at displayed price.         |
    /// | `currentGas == null` | Settled — winner set, cannot bid this cycle. |
    ///
    /// Example: `hypecli prio bid --keystore if_dev --ip 52.196.250.75 --max 1 --slot 0`
    Bid(BidCmd),
}

impl PrioCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Status(cmd) => cmd.run().await,
            Self::Bid(cmd) => cmd.run().await,
        }
    }
}

#[derive(Args)]
pub struct StatusCmd {
    #[arg(long, default_value = "mainnet")]
    pub chain: Chain,
}

impl StatusCmd {
    /// Fetch all 5 slots from `/info`.
    ///
    /// `currentGas != null` → auction RUNNING at displayed price.
    /// `currentGas == null` → settled, cannot bid this cycle.
    /// Prints `started <timestamp>` at the top so you know when the cycle began.
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let status = client.gossip_priority_auction_status().await?;

        if let Some(first) = status.first() {
            let started = chrono::DateTime::from_timestamp(first.start_time_seconds as i64, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| first.start_time_seconds.to_string());
            println!("started {}\n", started);
        }

        println!(
            "{:<6} {:>12} {:>12} {:>12}",
            "Slot", "Start", "Current", "End/Min"
        );
        println!("{}", "-".repeat(48));

        for (i, slot) in status.iter().enumerate() {
            let cur_str = slot.current_gas.map(|d| d.to_string());
            println!(
                "{:<6} {:>12} {:>12} {:>12}",
                i,
                slot.start_gas,
                cur_str.as_deref().unwrap_or("(no bid)"),
                slot.end_gas.map(|d| d.to_string()).as_deref().unwrap_or("-")
            );
        }

        Ok(())
    }
}

/// Place a signed bid on a gossip priority slot.
///
/// You pay the live price at TX mining time. If you win, any difference
/// between your `--max` and the actual cost is refunded automatically.
#[derive(Args, derive_more::Deref)]
pub struct BidCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Max HYPE to bid. You pay the live price at execution time, capped here.
    #[arg(long)]
    pub max: Decimal,

    /// IP address to receive prioritized gossip.
    #[arg(long)]
    pub ip: String,

    #[arg(long, default_value = "0")]
    pub slot: u8,
}

impl BidCmd {
    /// Fetch the slot's `currentGas` from `/info`.
    ///
    /// - If `currentGas >= --max`: skip (already outbid or at floor).
    /// - Otherwise bid `currentGas + 1` (or `--max` if no leader yet).
    ///
    /// You pay the live `currentGas` price at TX mining time — not `--max`.
    /// The difference is refunded automatically. Winning amount is burned.
    pub async fn run(self) -> anyhow::Result<()> {
        let signer = find_signer_sync(&self.signer)?;
        let client = HttpClient::new(self.chain);

        let decimals = client
            .spot_tokens()
            .await?
            .into_iter()
            .find(|t| t.name == "HYPE")
            .map(|t| t.wei_decimals as u32)
            .unwrap_or(18);

        let max_gas: u64 = hypersdk::hyperevm::to_wei(self.max, decimals)
            .try_into()
            .map_err(|_| anyhow::anyhow!("--max too large"))?;

        let status = client.gossip_priority_auction_status().await?;
        let slot = status
            .get(self.slot as usize)
            .ok_or_else(|| anyhow::anyhow!("invalid slot {}", self.slot))?;

        let current: u64 = slot
            .current_gas
            .and_then(|d| u64::try_from(d).ok())
            .unwrap_or(0);

        if current >= max_gas && current > 0 {
            println!(
                "Leader {} >= max {}; not bidding.",
                fmt_wei(current, decimals),
                self.max
            );
            return Ok(());
        }

        let bid = if current > 0 { current + 1 } else { max_gas };

        let nonce = NonceHandler::default().next();
        let resp = client
            .gossip_priority_bid(&signer, self.slot, &self.ip, bid, nonce, None, None)
            .await?;

        match &resp {
            Response::Ok(OkResponse::Default) => {
                println!("-- Bid {} on slot {}", fmt_wei(bid, decimals), self.slot);
            }
            Response::Err(e) => {
                println!("XX Error: {e}");
            }
            _ => {
                println!("?? {:?}", resp);
            }
        }

        Ok(())
    }
}

fn fmt_wei(wei: u64, decimals: u32) -> Decimal {
    Decimal::from_u64(wei).unwrap() / Decimal::from_u64(10u64.pow(decimals)).unwrap()
}
