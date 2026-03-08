//! Deposit or withdraw USDC from a Hyperliquid vault.
//!
//! # Usage
//!
//! ```bash
//! # Deposit 100 USDC
//! cargo run --example vault_transfer -- <VAULT_ADDRESS> deposit 100
//!
//! # Withdraw 50 USDC
//! cargo run --example vault_transfer -- <VAULT_ADDRESS> withdraw 50
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use hypersdk::{Address, hypercore};
use rust_decimal::Decimal;

use crate::credentials::Credentials;

mod credentials;

#[derive(Parser, Debug, derive_more::Deref)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[deref]
    #[command(flatten)]
    common: Credentials,
    /// Vault address
    #[arg(short, long)]
    vault: Address,
    /// Operation: "deposit" or "withdraw"
    #[arg(short, long)]
    operation: String,
    /// Amount of USDC
    #[arg(short, long)]
    amount: Decimal,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = simple_logger::init_with_level(log::Level::Debug);

    let args = Cli::parse();
    let signer = args.get()?;

    let client = hypercore::mainnet();

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    match args.operation.as_str() {
        "deposit" => {
            client.vault_transfer(&signer, args.vault, args.amount, nonce, true).await?;
            println!("Deposited ${} into vault {}", args.amount, args.vault);
        }
        "withdraw" => {
            client.vault_transfer(&signer, args.vault, args.amount, nonce, false).await?;
            println!("Withdrew ${} from vault {}", args.amount, args.vault);
        }
        op => anyhow::bail!("unknown operation '{op}', use 'deposit' or 'withdraw'"),
    }

    Ok(())
}
