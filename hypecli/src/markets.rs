//! Market data query commands.
//!
//! This module provides commands for querying perpetual and spot market information
//! from the Hyperliquid protocol.

use std::io::{Write, stdout};

use clap::Args;
use hypersdk::hypercore;

/// Command to list all perpetual futures markets.
///
/// Queries the Hyperliquid API for available perpetual markets and displays
/// their configuration including leverage limits and decimals.
///
/// # Example
///
/// ```bash
/// hypecli perps
/// ```
///
/// # Output
///
/// Displays a table with columns:
/// - `name`: Market symbol (e.g., BTC, ETH)
/// - `collateral`: Collateral token index
/// - `index`: Market index number
/// - `sz_decimals`: Size precision decimals
/// - `max leverage`: Maximum allowed leverage
/// - `isolated margin`: Maximum isolated margin percentage
#[derive(Args)]
pub struct PerpsCmd;

impl PerpsCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let core = hypercore::mainnet();
        let perps = core.perps().await?;
        let mut writer = tabwriter::TabWriter::new(stdout());

        let _ = writeln!(
            &mut writer,
            "name\tcollateral\tindex\tsz_decimals\tmax leverage\tisolated margin"
        );
        for perp in perps {
            let _ = writeln!(
                &mut writer,
                "{}\t{}\t{}\t{}\t{}\t{}",
                perp.name,
                perp.collateral,
                perp.index,
                perp.sz_decimals,
                perp.max_leverage,
                perp.isolated_margin,
            );
        }

        let _ = writer.flush();

        Ok(())
    }
}

/// Command to list all spot trading markets.
///
/// Queries the Hyperliquid API for available spot trading pairs and displays
/// their configuration including token addresses and market indices.
///
/// # Example
///
/// ```bash
/// hypecli spot
/// ```
///
/// # Output
///
/// Displays a table with columns:
/// - `pair`: Trading pair (BASE/QUOTE)
/// - `name`: Spot market name
/// - `index`: Market index number
/// - `base evm address`: EVM contract address for base token
/// - `quote evm address`: EVM contract address for quote token
#[derive(Args)]
pub struct SpotCmd;

impl SpotCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let core = hypercore::mainnet();
        let markets = core.spot().await?;
        let mut writer = tabwriter::TabWriter::new(stdout());

        writeln!(
            &mut writer,
            "pair\tname\tindex\tbase evm address\tquote evm address"
        )?;
        for spot in markets {
            writeln!(
                &mut writer,
                "{}/{}\t{}\t{}\t{:?}\t{:?}",
                spot.tokens[0].name,
                spot.tokens[1].name,
                spot.name,
                spot.index,
                spot.tokens[0].evm_contract,
                spot.tokens[1].evm_contract,
            )?;
        }

        writer.flush()?;

        Ok(())
    }
}
