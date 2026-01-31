//! User balance query commands.
//!
//! This module provides commands for querying user balances on Hyperliquid.

use std::io::{Write, stdout};

use clap::{Args, ValueEnum};
use hypersdk::{Address, hypercore};
use rust_decimal::Decimal;
use serde::Serialize;

/// Output format for balance data.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable formatted output
    #[default]
    Pretty,
    /// Tab-aligned table output
    Table,
    /// JSON output for programmatic consumption
    Json,
}

/// Serializable balance data for JSON output.
#[derive(Serialize)]
struct BalanceOutput {
    spot: Vec<SpotBalance>,
    perp: AccountState,
    dexes: Vec<DexState>,
}

#[derive(Serialize)]
struct SpotBalance {
    coin: String,
    hold: Decimal,
    total: Decimal,
}

#[derive(Serialize)]
struct AccountState {
    account_value: Decimal,
    margin_used: Decimal,
    withdrawable: Decimal,
    positions: Vec<Position>,
}

#[derive(Serialize)]
struct DexState {
    name: String,
    account_value: Decimal,
    margin_used: Decimal,
    withdrawable: Decimal,
    positions: Vec<Position>,
}

#[derive(Serialize)]
struct Position {
    coin: String,
    size: Decimal,
    entry_price: Option<Decimal>,
    unrealized_pnl: Decimal,
}

/// Command to query all balances for a user address.
///
/// Retrieves and displays spot, perp, and all DEX balances for the specified address.
///
/// # Example
///
/// ```bash
/// hypecli balance 0x1234567890abcdef1234567890abcdef12345678
/// hypecli balance 0x1234... --format table
/// hypecli balance 0x1234... --format json
/// ```
#[derive(Args)]
pub struct BalanceCmd {
    /// User address to query balances for.
    pub user: Address,

    /// Output format.
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
}

impl BalanceCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let core = hypercore::mainnet();

        // Query spot balances
        let spot_balances = core.user_balances(self.user).await?;

        // Query perp clearinghouse state
        let perp_state = core.clearinghouse_state(self.user, None).await?;

        // Query all DEXes
        let dexes = core.perp_dexs().await?;

        // Collect DEX states
        let mut dex_states = Vec::new();
        for dex in &dexes {
            let dex_name = dex.name();
            let state = core
                .clearinghouse_state(self.user, Some(dex_name.to_string()))
                .await?;
            dex_states.push((dex_name.to_string(), state));
        }

        match self.format {
            OutputFormat::Pretty => self.print_pretty(&spot_balances, &perp_state, &dex_states)?,
            OutputFormat::Table => self.print_table(&spot_balances, &perp_state, &dex_states)?,
            OutputFormat::Json => self.print_json(&spot_balances, &perp_state, &dex_states)?,
        }

        Ok(())
    }

    fn print_pretty(
        &self,
        spot_balances: &[hypersdk::hypercore::types::UserBalance],
        perp_state: &hypersdk::hypercore::types::ClearinghouseState,
        dex_states: &[(String, hypersdk::hypercore::types::ClearinghouseState)],
    ) -> anyhow::Result<()> {
        // Spot balances
        println!("=== Spot Balances ===");
        if spot_balances.is_empty() {
            println!("(no spot balances)");
        } else {
            for balance in spot_balances {
                println!(
                    "  {}: {} (hold: {})",
                    balance.coin, balance.total, balance.hold
                );
            }
        }
        println!();

        // Perp account
        println!("=== Perp Account ===");
        println!(
            "  Account value: {}",
            perp_state.margin_summary.account_value
        );
        println!(
            "  Margin used:   {}",
            perp_state.margin_summary.total_margin_used
        );
        println!("  Withdrawable:  {}", perp_state.withdrawable);

        if !perp_state.asset_positions.is_empty() {
            println!("  Positions:");
            for pos in &perp_state.asset_positions {
                let entry_px = pos
                    .position
                    .entry_px
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "    {}: {} @ {} (pnl: {})",
                    pos.position.coin, pos.position.szi, entry_px, pos.position.unrealized_pnl
                );
            }
        }
        println!();

        // DEX states
        for (name, state) in dex_states {
            println!("=== DEX: {} ===", name);
            println!("  Account value: {}", state.margin_summary.account_value);
            println!(
                "  Margin used:   {}",
                state.margin_summary.total_margin_used
            );
            println!("  Withdrawable:  {}", state.withdrawable);

            if !state.asset_positions.is_empty() {
                println!("  Positions:");
                for pos in &state.asset_positions {
                    let entry_px = pos
                        .position
                        .entry_px
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    println!(
                        "    {}: {} @ {} (pnl: {})",
                        pos.position.coin, pos.position.szi, entry_px, pos.position.unrealized_pnl
                    );
                }
            }
            println!();
        }

        Ok(())
    }

    fn print_table(
        &self,
        spot_balances: &[hypersdk::hypercore::types::UserBalance],
        perp_state: &hypersdk::hypercore::types::ClearinghouseState,
        dex_states: &[(String, hypersdk::hypercore::types::ClearinghouseState)],
    ) -> anyhow::Result<()> {
        let mut writer = tabwriter::TabWriter::new(stdout());

        // Header
        writeln!(&mut writer, "venue\tcoin\tavailable\thold")?;

        // Spot balances
        for balance in spot_balances {
            let available = balance.total - balance.hold;
            writeln!(
                &mut writer,
                "spot\t{}\t{}\t{}",
                balance.coin, available, balance.hold
            )?;
        }

        // Perp account - available is withdrawable, hold is account_value - withdrawable
        let perp_available = perp_state.withdrawable;
        let perp_hold = perp_state.margin_summary.account_value - perp_state.withdrawable;
        writeln!(&mut writer, "perp\tUSD\t{}\t{}", perp_available, perp_hold)?;

        // DEX states
        for (name, state) in dex_states {
            let dex_available = state.withdrawable;
            let dex_hold = state.margin_summary.account_value - state.withdrawable;
            // Only show if there's any balance
            if state.margin_summary.account_value != Decimal::ZERO {
                writeln!(
                    &mut writer,
                    "{}\tUSD\t{}\t{}",
                    name, dex_available, dex_hold
                )?;
            }
        }

        writer.flush()?;
        Ok(())
    }

    fn print_json(
        &self,
        spot_balances: &[hypersdk::hypercore::types::UserBalance],
        perp_state: &hypersdk::hypercore::types::ClearinghouseState,
        dex_states: &[(String, hypersdk::hypercore::types::ClearinghouseState)],
    ) -> anyhow::Result<()> {
        let output = BalanceOutput {
            spot: spot_balances
                .iter()
                .map(|b| SpotBalance {
                    coin: b.coin.clone(),
                    hold: b.hold,
                    total: b.total,
                })
                .collect(),
            perp: AccountState {
                account_value: perp_state.margin_summary.account_value,
                margin_used: perp_state.margin_summary.total_margin_used,
                withdrawable: perp_state.withdrawable,
                positions: perp_state
                    .asset_positions
                    .iter()
                    .map(|p| Position {
                        coin: p.position.coin.clone(),
                        size: p.position.szi,
                        entry_price: p.position.entry_px,
                        unrealized_pnl: p.position.unrealized_pnl,
                    })
                    .collect(),
            },
            dexes: dex_states
                .iter()
                .map(|(name, state)| DexState {
                    name: name.clone(),
                    account_value: state.margin_summary.account_value,
                    margin_used: state.margin_summary.total_margin_used,
                    withdrawable: state.withdrawable,
                    positions: state
                        .asset_positions
                        .iter()
                        .map(|p| Position {
                            coin: p.position.coin.clone(),
                            size: p.position.szi,
                            entry_price: p.position.entry_px,
                            unrealized_pnl: p.position.unrealized_pnl,
                        })
                        .collect(),
                })
                .collect(),
        };

        println!("{}", serde_json::to_string_pretty(&output)?);
        Ok(())
    }
}
