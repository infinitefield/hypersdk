//! User balance query commands.
//!
//! This module provides commands for querying user balances on Hyperliquid.

use std::io::{Write, stdout};

use clap::Args;
use hypersdk::{Address, hypercore};

/// Command to query spot balances for a user address.
///
/// Retrieves and displays all spot token balances for the specified address,
/// including held amounts (locked in orders) and total balances.
///
/// # Example
///
/// ```bash
/// hypecli spot-balances --user 0x1234567890abcdef1234567890abcdef12345678
/// ```
///
/// # Output
///
/// Displays a table with columns:
/// - `coin`: Token symbol
/// - `hold`: Amount locked in orders or other operations
/// - `total`: Total balance (including held amount)
///
/// Available balance = total - hold
#[derive(Args)]
pub struct SpotBalancesCmd {
    /// User address to query balances for.
    #[arg(short, long)]
    pub user: Address,
}

impl SpotBalancesCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let core = hypercore::mainnet();
        let balances = core.user_balances(self.user).await?;
        let mut writer = tabwriter::TabWriter::new(stdout());

        writeln!(&mut writer, "coin\thold\ttotal")?;
        for balance in balances {
            writeln!(
                &mut writer,
                "{}\t{}\t{}",
                balance.coin, balance.hold, balance.total
            )?;
        }

        writer.flush()?;

        Ok(())
    }
}
