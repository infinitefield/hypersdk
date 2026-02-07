//! Asset transfer commands.
//!
//! This module provides commands for sending assets between accounts,
//! DEXes, and subaccounts on Hyperliquid.

use alloy::primitives::Address;
use clap::Args;
use hypersdk::{
    Decimal,
    hypercore::{self, AssetTarget, HttpClient, NonceHandler, SendAsset, SendToken},
};

use crate::SignerArgs;
use crate::utils::find_signer_sync;

/// Send assets between accounts or DEXes.
///
/// This command allows transferring tokens between:
/// - Different users (perp to perp, spot to spot)
/// - Different balances (perp to spot, spot to perp)
/// - Different DEXes (HIP-3)
/// - Subaccounts
///
/// # Examples
///
/// Send USDC from perp to spot balance (same user):
/// ```bash
/// hypecli send --private-key <KEY> --token USDC --amount 100 --from perp --to spot
/// ```
///
/// Send USDC to another address:
/// ```bash
/// hypecli send --private-key <KEY> --token USDC --amount 100 --destination 0x1234...
/// ```
///
/// Send HYPE from spot to another user's spot:
/// ```bash
/// hypecli send --private-key <KEY> --token HYPE --amount 50 --from spot --to spot --destination 0x1234...
/// ```
///
/// Transfer between HIP-3 DEXes:
/// ```bash
/// hypecli send --private-key <KEY> --token USDC --amount 100 --from perp --to xyz
/// ```
#[derive(Args, derive_more::Deref)]
pub struct SendCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Token to send (symbol name, e.g., "USDC", "HYPE", "PURR")
    #[arg(long)]
    pub token: String,

    /// Amount to send
    #[arg(long)]
    pub amount: Decimal,

    /// Destination address (defaults to self for internal transfers)
    #[arg(long)]
    pub destination: Option<Address>,

    /// Source location: "perp", "spot", or a HIP-3 DEX name
    #[arg(long, default_value = "perp")]
    pub from: AssetTarget,

    /// Destination location: "perp", "spot", or a HIP-3 DEX name
    #[arg(long, default_value = "perp")]
    pub to: AssetTarget,

    /// Source subaccount name (if sending from a subaccount)
    #[arg(long)]
    pub from_subaccount: Option<String>,
}

impl SendCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let signer = find_signer_sync(&self.signer)?;
        let client = HttpClient::new(self.chain);

        // Find the token
        let tokens = hypercore::mainnet().spot_tokens().await?;
        let token = tokens
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(&self.token))
            .ok_or_else(|| anyhow::anyhow!("Token '{}' not found", self.token))?;

        // If no destination specified, send to self (for internal transfers)
        let destination = self.destination.unwrap_or_else(|| signer.address());

        let nonce = NonceHandler::default().next();

        let send = SendAsset {
            destination,
            source_dex: self.from.clone(),
            destination_dex: self.to.clone(),
            token: SendToken(token.clone()),
            amount: self.amount,
            from_sub_account: self.from_subaccount.clone().unwrap_or_default(),
            nonce,
        };

        println!(
            "Sending {} {} from {} to {}",
            self.amount, self.token, self.from, self.to
        );
        println!("  From: {}", signer.address());
        println!("  To:   {}", destination);
        if let Some(ref sub) = self.from_subaccount {
            println!("  Subaccount: {}", sub);
        }

        client.send_asset(&signer, send, nonce).await?;

        println!("Success!");

        Ok(())
    }
}
