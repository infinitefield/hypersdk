//! Place and optionally cancel a TWAP order.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example twap-order -- --coin BTC --side buy --sz 0.01 --minutes 30 --randomize
//! ```

use clap::{Parser, ValueEnum};
use hypersdk::{
    Decimal,
    hypercore::{
        self as hypercore, NonceHandler, TwapCancel, TwapOrder,
        api::{TwapCancelStatus, TwapOrderStatus},
    },
};

use crate::credentials::Credentials;

mod credentials;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SideArg {
    Buy,
    Sell,
}

#[derive(Parser, Debug)]
#[command(author, version, about = "Place and optionally cancel a TWAP order")]
struct Cli {
    #[command(flatten)]
    common: Credentials,
    /// Perp symbol, e.g. BTC, ETH, HYPE.
    #[arg(long, default_value = "BTC")]
    coin: String,
    /// TWAP side.
    #[arg(long, value_enum, default_value_t = SideArg::Buy)]
    side: SideArg,
    /// Total TWAP size in base units.
    #[arg(long)]
    sz: Decimal,
    /// TWAP duration in minutes.
    #[arg(long)]
    minutes: u64,
    /// Whether the TWAP should only reduce an existing position.
    #[arg(long, default_value_t = false)]
    reduce_only: bool,
    /// Whether to randomize child slices.
    #[arg(long, default_value_t = false)]
    randomize: bool,
    /// Cancel the TWAP immediately after successful submit.
    #[arg(long, default_value_t = false)]
    cancel_after_submit: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = simple_logger::init_with_level(log::Level::Info);

    let args = Cli::parse();
    let signer = args.common.get()?;

    let client = hypercore::mainnet();
    let role = client.user_role(signer.address()).await?;

    let perps = client.perps().await?;
    let market = perps
        .iter()
        .find(|perp| perp.name.eq_ignore_ascii_case(&args.coin))
        .ok_or_else(|| anyhow::anyhow!("perp market not found: {}", args.coin))?;

    let vault_address = match role {
        hypercore::UserRole::SubAccount { master } => Some(master),
        _ => None,
    };

    let nonce = NonceHandler::default();
    let twap = TwapOrder {
        asset: market.index,
        is_buy: matches!(args.side, SideArg::Buy),
        sz: args.sz,
        reduce_only: args.reduce_only,
        minutes: args.minutes,
        randomize: args.randomize,
    };

    let status = client
        .twap_order(&signer, twap, nonce.next(), vault_address, None)
        .await?;

    match status {
        TwapOrderStatus::Running { running } => {
            println!("TWAP started: twapId={}", running.twap_id);

            if args.cancel_after_submit {
                let cancel_status = client
                    .twap_cancel(
                        &signer,
                        TwapCancel {
                            asset: market.index,
                            twap_id: running.twap_id,
                        },
                        nonce.next(),
                        vault_address,
                        None,
                    )
                    .await?;

                match cancel_status {
                    TwapCancelStatus::Success(_) => {
                        println!("TWAP cancelled: twapId={}", running.twap_id);
                    }
                    TwapCancelStatus::Error { error } => {
                        anyhow::bail!("twap cancel error: {error}");
                    }
                    TwapCancelStatus::Unknown(raw) => {
                        anyhow::bail!("twap cancel unknown response: {raw}");
                    }
                }
            }
        }
        TwapOrderStatus::Error { error } => {
            anyhow::bail!("twap order error: {error}");
        }
        TwapOrderStatus::Unknown(raw) => {
            anyhow::bail!("twap order unknown response: {raw}");
        }
    }

    Ok(())
}
