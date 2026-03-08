//! Vault transfer commands.
//!
//! This module provides commands for depositing and withdrawing USDC
//! from Hyperliquid vaults.

use alloy::primitives::Address;
use clap::{Args, Subcommand};
use hypersdk::{Decimal, hypercore::{HttpClient, NonceHandler}};

use crate::SignerArgs;
use crate::utils::find_signer_sync;

/// Vault deposit and withdrawal commands.
#[derive(Subcommand)]
pub enum VaultCmd {
    /// Deposit USDC into a vault
    Deposit(VaultTransferCmd),
    /// Withdraw USDC from a vault
    Withdraw(VaultTransferCmd),
}

impl VaultCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let (cmd, is_deposit) = match self {
            VaultCmd::Deposit(cmd) => (cmd, true),
            VaultCmd::Withdraw(cmd) => (cmd, false),
        };

        let signer = find_signer_sync(&cmd.signer)?;
        let client = HttpClient::new(cmd.signer.chain);
        let nonce = NonceHandler::default().next();

        let (verb, past) = if is_deposit { ("Depositing", "Deposited") } else { ("Withdrawing", "Withdrawn") };
        println!("{} ${} vault {}", verb, cmd.amount, cmd.vault);
        client.vault_transfer(&signer, cmd.vault, cmd.amount, nonce, is_deposit).await?;
        println!("{} successfully.", past);

        Ok(())
    }
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
