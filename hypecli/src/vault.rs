//! Vault transfer commands.
//!
//! This module provides commands for depositing and withdrawing USDC
//! from Hyperliquid vaults.

use alloy::primitives::Address;
use clap::{Args, Subcommand};
use hypersdk::{Decimal, hypercore::{self, HttpClient, NonceHandler}};

use crate::SignerArgs;
use crate::utils::find_signer_sync;

/// Vault deposit and withdrawal commands.
#[derive(Subcommand)]
pub enum VaultCmd {
    /// Deposit USDC into a vault
    Deposit(VaultTransferCmd),
    /// Withdraw USDC from a vault
    Withdraw(VaultTransferCmd),
    /// Query details for a vault
    Details(VaultDetailsCmd),
}

impl VaultCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            VaultCmd::Details(cmd) => cmd.run().await,
            VaultCmd::Deposit(cmd) => execute_transfer(cmd, true).await,
            VaultCmd::Withdraw(cmd) => execute_transfer(cmd, false).await,
        }
    }
}

async fn execute_transfer(cmd: VaultTransferCmd, is_deposit: bool) -> anyhow::Result<()> {
    let (verb, past) = if is_deposit { ("Depositing", "Deposited") } else { ("Withdrawing", "Withdrawn") };
    let signer = find_signer_sync(&cmd.signer)?;
    let client = HttpClient::new(cmd.signer.chain);
    let nonce = NonceHandler::default().next();
    println!("{} ${} vault {}", verb, cmd.amount, cmd.vault);
    client.vault_transfer(&signer, cmd.vault, cmd.amount, nonce, is_deposit).await?;
    println!("{} successfully.", past);
    Ok(())
}

/// Arguments for vault deposit and withdrawal.
#[derive(Args, derive_more::Deref)]
pub struct VaultTransferCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Vault address to deposit into or withdraw from
    #[arg(long)]
    pub vault: Address,

    /// Amount of USDC to transfer
    #[arg(long)]
    pub amount: Decimal,
}

/// Arguments for vault details query.
#[derive(Args)]
pub struct VaultDetailsCmd {
    /// Vault address to query
    #[arg(long)]
    pub vault: Address,

    /// Optional user address to include follower state
    #[arg(long)]
    pub user: Option<Address>,
}

impl VaultDetailsCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = hypercore::mainnet();
        let details = client.vault_details(self.vault, self.user).await?;

        println!("Vault: {}", details.name);
        println!("Address: {:?}", details.vault_address);
        println!("Leader: {:?}", details.leader);
        println!("Description: {}", details.description);
        println!();
        println!("APR: {}%", details.apr * Decimal::ONE_HUNDRED);
        println!("Leader Fraction: {}%", details.leader_fraction * Decimal::ONE_HUNDRED);
        println!("Leader Commission: {}%", details.leader_commission * Decimal::ONE_HUNDRED);
        println!("Max Distributable: ${}", details.max_distributable);
        println!("Max Withdrawable: ${}", details.max_withdrawable);
        println!();
        println!("Followers: {}", details.followers.len());
        const DAY_PERIOD: &str = "day";
        let tvl = details.portfolio.iter()
            .find(|(period, _)| period == DAY_PERIOD)
            .and_then(|(_, p)| p.account_value_history.iter().max_by_key(|(ts, _)| *ts))
            .map(|(_, value)| value.as_str());
        if let Some(tvl) = tvl {
            println!("TVL: ${}", tvl);
        }

        if let Some(state) = details.follower_state {
            println!();
            println!("Your Position:");
            println!("  Equity: ${}", state.vault_equity);
            println!("  PnL: ${}", state.pnl);
            println!("  All-time PnL: ${}", state.all_time_pnl);
            println!("  Days Following: {}", state.days_following);
            if let Some(lockup) = state.lockup_until {
                println!("  Locked Until: {}", lockup);
            }
        }

        Ok(())
    }
}
