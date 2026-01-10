//! Morpho protocol query commands.
//!
//! This module provides commands for querying positions on the Morpho lending
//! protocol deployed on HyperEVM.

use std::io::{Write, stdout};

use clap::Args;
use hypersdk::{Address, hyperevm, hyperevm::morpho};

/// Command to query a user's position in a Morpho lending market.
///
/// Queries the Morpho protocol on HyperEVM to retrieve a user's position data,
/// including borrow shares, collateral, and supply shares for a specific market.
///
/// # Example
///
/// ```bash
/// hypecli morpho-position \
///   --user 0x1234567890abcdef1234567890abcdef12345678 \
///   --market 0xabcd...1234
/// ```
///
/// # Output
///
/// Displays a table with columns:
/// - `borrow shares`: Amount of borrow shares held
/// - `collateral`: Collateral amount deposited
/// - `supply shares`: Amount of supply shares held
///
/// # Optional Arguments
///
/// - `--contract`: Morpho contract address (default: mainnet address)
/// - `--rpc-url`: Custom RPC endpoint (default: Hyperliquid mainnet)
#[derive(Args)]
pub struct MorphoPositionCmd {
    /// Morpho's contract address.
    #[arg(
        short,
        long,
        default_value = "0x68e37dE8d93d3496ae143F2E900490f6280C57cD"
    )]
    pub contract: Address,
    /// RPC endpoint URL for HyperEVM.
    #[arg(short, long, default_value = "https://rpc.hyperliquid.xyz/evm")]
    pub rpc_url: String,
    /// Morpho market ID to query.
    #[arg(short, long)]
    pub market: morpho::MarketId,
    /// Target user address.
    #[arg(short, long)]
    pub user: Address,
}

impl MorphoPositionCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let provider = hyperevm::mainnet_with_url(&self.rpc_url).await?;
        let client = hyperevm::morpho::Client::new(provider);
        let morpho = client.instance(self.contract);
        let position = morpho.position(self.market, self.user).call().await?;

        let mut writer = tabwriter::TabWriter::new(stdout());

        writeln!(&mut writer, "borrow shares\tcollateral\tsupply shares")?;
        writeln!(
            &mut writer,
            "{}\t{}\t{}",
            position.borrowShares, position.collateral, position.supplyShares
        )?;

        writer.flush()?;

        Ok(())
    }
}
