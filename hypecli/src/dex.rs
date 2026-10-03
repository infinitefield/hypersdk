//! HIP-3 DEX deployer commands.
//!
//! This module provides commands for inspecting a HIP-3 DEX's configuration,
//! halting and resuming trading on a coin, delegating deployer actions to
//! sub-deployers, and managing the allowlist of testnet-only HIP-3* venues.

use std::{
    collections::BTreeMap,
    io::{Write, stdout},
};

use alloy::primitives::Address;
use clap::{Args, Subcommand};
use hypersdk::{
    Decimal,
    hypercore::{
        Chain, HttpClient, SubDeployerPermission,
        api::Action,
        deploy::{
            HaltTrading, Hip3StarAction, Hip3StarCancelAll, Hip3StarOperation,
            Hip3StarProxyOperation, PerpDeployAction, SetSubDeployers, SubDeployerInput,
        },
    },
};

use crate::action::ActionArgs;

/// HIP-3 DEX deployer commands.
#[derive(Subcommand)]
pub enum DexCmd {
    /// Show a DEX's deployer, sub-deployers, and per-coin settings
    Info(DexInfoCmd),
    /// Halt trading on a coin
    Halt(DexHaltCmd),
    /// Resume trading on a halted coin
    Resume(DexHaltCmd),
    /// Grant or revoke a sub-deployer's permission to send one deployer action
    SubDeployer(DexSubDeployerCmd),
    /// Add a user to a HIP-3* venue's allowlist (testnet-only)
    Allow(DexStarUserCmd),
    /// Remove a user from a HIP-3* venue's allowlist, clearing their flags (testnet-only)
    Disallow(DexStarUserCmd),
    /// Restrict an approved user of a HIP-3* venue to reducing positions (testnet-only)
    ReduceOnly(DexReduceOnlyCmd),
    /// Cancel a user's resting orders and TWAPs on a HIP-3* venue (testnet-only)
    CancelAll(DexStarUserCmd),
    /// Show a user's approval and flags on every HIP-3* venue
    StarState(DexStarStateCmd),
}

impl DexCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            DexCmd::Info(cmd) => cmd.run().await,
            DexCmd::Halt(cmd) => execute_halt(cmd, true).await,
            DexCmd::Resume(cmd) => execute_halt(cmd, false).await,
            DexCmd::SubDeployer(cmd) => cmd.run().await,
            DexCmd::Allow(cmd) => execute_approval(cmd, true).await,
            DexCmd::Disallow(cmd) => execute_approval(cmd, false).await,
            DexCmd::ReduceOnly(cmd) => cmd.run().await,
            DexCmd::CancelAll(cmd) => execute_cancel_all(cmd).await,
            DexCmd::StarState(cmd) => cmd.run().await,
        }
    }
}

/// Arguments for the DEX configuration query.
#[derive(Args)]
pub struct DexInfoCmd {
    /// DEX name, e.g. `xyz`
    #[arg(long)]
    pub dex: String,

    /// Chain to query.
    #[arg(long, default_value = "mainnet")]
    pub chain: Chain,
}

impl DexInfoCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let dex = client
            .perp_dex_details()
            .await?
            .into_iter()
            .find(|dex| dex.name.eq_ignore_ascii_case(&self.dex))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "HIP-3 DEX '{}' not found. Use 'hypecli dexes' to list available DEXes.",
                    self.dex
                )
            })?;
        let status = client.perp_dex_status(dex.name.clone()).await?;

        let or_dash = |address: Option<Address>| {
            address.map_or_else(|| "-".to_string(), |address| address.to_string())
        };

        let mut writer = tabwriter::TabWriter::new(stdout());

        let _ = writeln!(&mut writer, "name\t{}", dex.name);
        let _ = writeln!(&mut writer, "full name\t{}", dex.full_name);
        let _ = writeln!(&mut writer, "index\t{}", dex.index);
        let _ = writeln!(&mut writer, "deployer\t{}", dex.deployer);
        let _ = writeln!(
            &mut writer,
            "oracle updater\t{}",
            or_dash(dex.oracle_updater)
        );
        let _ = writeln!(&mut writer, "fee recipient\t{}", or_dash(dex.fee_recipient));
        let _ = writeln!(&mut writer, "net deposit\t{}", status.total_net_deposit);

        let _ = writeln!(&mut writer);
        let _ = writeln!(&mut writer, "permission\tsub-deployers");
        for grant in &dex.sub_deployers {
            let users = grant
                .users
                .iter()
                .map(Address::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(&mut writer, "{}\t{}", grant.permission, users);
        }

        // One row per coin with any setting. A coin without an entry uses the default.
        let mut coins: BTreeMap<&str, [Option<Decimal>; 4]> = BTreeMap::new();
        let settings = [
            &dex.asset_to_streaming_oi_cap,
            &dex.asset_to_funding_multiplier,
            &dex.asset_to_funding_interest_rate,
            &dex.asset_to_funding_clamp,
        ];
        for (column, setting) in settings.into_iter().enumerate() {
            for entry in setting {
                coins.entry(entry.coin.as_str()).or_default()[column] = Some(entry.value);
            }
        }

        let _ = writeln!(&mut writer);
        let _ = writeln!(
            &mut writer,
            "coin\toi cap\tfunding multiplier\tfunding interest rate\tfunding clamp"
        );
        for (coin, values) in coins {
            let [oi_cap, multiplier, interest_rate, clamp] =
                values.map(|value| value.map_or_else(|| "-".to_string(), |v| v.to_string()));
            let _ = writeln!(
                &mut writer,
                "{coin}\t{oi_cap}\t{multiplier}\t{interest_rate}\t{clamp}"
            );
        }

        let _ = writer.flush();

        Ok(())
    }
}

/// Arguments for halting or resuming trading on a coin.
#[derive(Args, derive_more::Deref)]
pub struct DexHaltCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// Coin with its DEX prefix, e.g. `xyz:SP500`
    #[arg(long)]
    pub coin: String,
}

async fn execute_halt(cmd: DexHaltCmd, is_halted: bool) -> anyhow::Result<()> {
    if !cmd.coin.contains(':') {
        anyhow::bail!(
            "'{}' has no DEX prefix. HIP-3 coins are named <dex>:<coin>, e.g. xyz:SP500",
            cmd.coin
        );
    }
    let (verb, past) = if is_halted {
        ("Halting", "Halted")
    } else {
        ("Resuming", "Resumed")
    };
    let client = HttpClient::new(cmd.signer.chain);
    println!("{} trading on {}", verb, cmd.coin);
    cmd.signer
        .execute_default(client, |_, _| {
            Action::PerpDeploy(PerpDeployAction::HaltTrading(HaltTrading {
                coin: cmd.coin,
                is_halted,
            }))
        })
        .await?;
    println!("{} successfully.", past);
    Ok(())
}

/// Arguments for granting or revoking a sub-deployer permission.
#[derive(Args, derive_more::Deref)]
pub struct DexSubDeployerCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// DEX name, e.g. `xyz`
    #[arg(long)]
    pub dex: String,

    /// Sub-deployer address
    #[arg(long)]
    pub user: Address,

    /// Deployer action to delegate, e.g. `setOracle` or `haltTrading`. HIP-3*
    /// operations take a `hip3Star:` prefix, e.g. `hip3Star:modifyApproval`
    #[arg(long, value_parser = parse_permission)]
    pub permission: SubDeployerPermission,

    /// Revoke the permission instead of granting it
    #[arg(long)]
    pub revoke: bool,
}

impl DexSubDeployerCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.signer.chain);
        let (verb, past) = if self.revoke {
            ("Revoking", "Revoked")
        } else {
            ("Granting", "Granted")
        };
        println!(
            "{} {} on {} for {}",
            verb, self.permission, self.dex, self.user
        );
        self.signer
            .execute_default(client, |_, _| {
                Action::PerpDeploy(PerpDeployAction::SetSubDeployers(SetSubDeployers {
                    dex: self.dex,
                    sub_deployers: vec![SubDeployerInput {
                        variant: self.permission,
                        user: self.user,
                        allowed: !self.revoke,
                    }],
                }))
            })
            .await?;
        println!("{} successfully.", past);
        Ok(())
    }
}

/// Reads `hip3Star:<operation>` as a HIP-3* grant and anything else as a `perpDeploy`
/// variant, the same form `dex info` prints.
fn parse_permission(permission: &str) -> Result<SubDeployerPermission, String> {
    match permission.strip_prefix("hip3Star:") {
        Some("") => Err("hip3Star: needs an operation, e.g. hip3Star:modifyApproval".into()),
        Some(action) => Ok(SubDeployerPermission::Hip3Star {
            action: action.to_owned(),
        }),
        None if permission.is_empty() => Err("permission cannot be empty".into()),
        None => Ok(permission.into()),
    }
}

/// Arguments for a HIP-3* operation on one user.
#[derive(Args, derive_more::Deref)]
pub struct DexStarUserCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// HIP-3* venue name
    #[arg(long)]
    pub dex: String,

    /// User to act on
    #[arg(long)]
    pub user: Address,
}

/// Arguments for setting or clearing a HIP-3* user's reduce-only flag.
#[derive(Args, derive_more::Deref)]
pub struct DexReduceOnlyCmd {
    #[deref]
    #[command(flatten)]
    pub star: DexStarUserCmd,

    /// Clear the flag and restore full trading
    #[arg(long)]
    pub off: bool,
}

impl DexReduceOnlyCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.star.signer.chain);
        if self.off {
            println!(
                "Restoring full trading for {} on {}",
                self.star.user, self.star.dex
            );
        } else {
            println!(
                "Setting {} to reduce-only on {}",
                self.star.user, self.star.dex
            );
        }
        let reduce_only = !self.off;
        let DexStarUserCmd { signer, dex, user } = self.star;
        signer
            .execute_default(client, |_, _| {
                star(
                    dex,
                    user,
                    Hip3StarProxyOperation::SetReduceOnly(reduce_only),
                )
            })
            .await?;
        println!("Updated successfully.");
        Ok(())
    }
}

async fn execute_approval(cmd: DexStarUserCmd, approved: bool) -> anyhow::Result<()> {
    let (verb, past) = if approved {
        ("Allowing", "Allowed")
    } else {
        ("Disallowing", "Disallowed")
    };
    let client = HttpClient::new(cmd.signer.chain);
    println!("{} {} on {}", verb, cmd.user, cmd.dex);
    cmd.signer
        .execute_default(client, |_, _| {
            star(
                cmd.dex,
                cmd.user,
                Hip3StarProxyOperation::ModifyApproval(approved),
            )
        })
        .await?;
    println!("{} successfully.", past);
    Ok(())
}

async fn execute_cancel_all(cmd: DexStarUserCmd) -> anyhow::Result<()> {
    let client = HttpClient::new(cmd.signer.chain);
    println!(
        "Cancelling all orders and TWAPs of {} on {}",
        cmd.user, cmd.dex
    );
    cmd.signer
        .execute_default(client, |_, _| {
            star(
                cmd.dex,
                cmd.user,
                Hip3StarProxyOperation::CancelAll(Hip3StarCancelAll::default()),
            )
        })
        .await?;
    println!("Cancelled successfully.");
    Ok(())
}

/// A HIP-3* operation applied to one user of the venue.
fn star(dex: String, user: Address, operation: Hip3StarProxyOperation) -> Action {
    Action::PerpDeploy(PerpDeployAction::Star(Hip3StarAction {
        dex,
        operation: Hip3StarOperation::Proxy(user, operation),
    }))
}

/// Arguments for the HIP-3* user state query.
#[derive(Args)]
pub struct DexStarStateCmd {
    /// User address
    pub user: Address,

    /// Chain to query.
    #[arg(long, default_value = "mainnet")]
    pub chain: Chain,
}

impl DexStarStateCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let state = client.user_star_state(self.user).await?;

        if state.dex_to_state.is_empty() {
            println!("No HIP-3* venue has approved {}.", self.user);
            if self.chain.is_mainnet() {
                println!("HIP-3* venues are testnet-only; try --chain testnet.");
            }
            return Ok(());
        }

        let mut writer = tabwriter::TabWriter::new(stdout());

        let _ = writeln!(
            &mut writer,
            "dex\tapproved\treduce only\tbackstop liquidator"
        );
        for venue in &state.dex_to_state {
            // No flags means the venue has since removed the user's approval.
            let (approved, reduce_only, backstop) = match venue.flags {
                Some(flags) => (
                    "yes",
                    flags.is_reduce_only.to_string(),
                    flags.is_backstop_liquidator_deposit_allowed.to_string(),
                ),
                None => ("no", "-".to_string(), "-".to_string()),
            };
            let _ = writeln!(
                &mut writer,
                "{}\t{}\t{}\t{}",
                venue.dex, approved, reduce_only, backstop
            );
        }

        let _ = writer.flush();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_parse_as_dex_info_prints_them() {
        for permission in ["setOracle", "haltTrading", "hip3Star:modifyApproval"] {
            let parsed = parse_permission(permission).unwrap();
            assert_eq!(parsed.to_string(), permission);
        }
        assert_eq!(
            parse_permission("hip3Star:cancelAll").unwrap(),
            SubDeployerPermission::Hip3Star {
                action: "cancelAll".into()
            }
        );
        assert!(parse_permission("").is_err());
        assert!(parse_permission("hip3Star:").is_err());
    }

    #[test]
    fn star_operations_use_the_documented_shape() {
        let user: Address = "0x1111111111111111111111111111111111111111"
            .parse()
            .unwrap();
        let action = star(
            "test".into(),
            user,
            Hip3StarProxyOperation::ModifyApproval(true),
        );
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            serde_json::json!({
                "type": "perpDeploy",
                "star": {
                    "dex": "test",
                    "operation": {"proxy": [
                        "0x1111111111111111111111111111111111111111",
                        {"modifyApproval": true}
                    ]}
                }
            })
        );
    }
}
