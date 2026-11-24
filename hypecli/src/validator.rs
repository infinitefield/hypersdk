//! Hyperliquid validator and staking commands.
//!
//! Hyperliquid validators separate two wallets (`hyperliquid-dex/node`):
//! - **Validator cold wallet (`validator`)**: holds staked HYPE, receives commission/rewards,
//!   and signs `CValidatorAction` (`register`, `changeProfile`, `unregister`), staking transfers
//!   (`cDeposit` / `cDeposit2` / `cWithdraw`), delegations (`tokenDelegate`), and agent approvals
//!   (`approveAgent`). Supports HyperCore multi-sig via `--multi-sig-addr`.
//! - **Consensus hot wallet (`signer`)**: configured in `~/hl/hyperliquid_data/node_config.json`
//!   as a single private key to sign HyperBFT consensus blocks/votes and `CSignerAction`
//!   (`jailSelf` / `unjailSelf`).

use std::{
    io::{Write, stdout},
    net::IpAddr,
};

use alloy::primitives::{Address, U256};
use anyhow::{Context, bail, ensure};
use clap::{Args, Subcommand};
use hypersdk::{
    Decimal,
    hypercore::{
        Chain, HttpClient,
        api::{
            Action, ApproveAgent, CDepositAction, CSignerAction, CValidatorAction,
            CWithdrawAction, TokenDelegateAction, ValidatorL1Stream, ValidatorNodeIp,
            ValidatorProfile, ValidatorProfileChange, ValidatorRegistration,
        },
    },
    hyperevm::{from_wei, to_wei},
};
use serde::Deserialize;

use crate::action::ActionArgs;

/// Staking native HYPE uses 8 decimals on HyperCore (`1 HYPE = 10^8 wei`).
const STAKING_WEI_DECIMALS: u32 = 8;
const MAX_COMMISSION_BPS: u64 = 10_000;

/// Validator inspection, profile/signer management, staking, and agent operations.
#[derive(Subcommand)]
pub enum ValidatorCmd {
    /// List validators on the network with stake, commission, jail status, and uptime
    List(ValidatorListCmd),
    /// Show detailed configuration, multisig signers, delegations, and agents for one validator
    Info(ValidatorInfoCmd),
    /// Rotate a validator's consensus signer hot wallet (`CValidatorAction::ChangeProfile`)
    RotateSigner(RotateSignerCmd),
    /// Update a validator's profile (`CValidatorAction::ChangeProfile`)
    ChangeProfile(ChangeProfileCmd),
    /// Register a new validator and self-delegate initial stake (`CValidatorAction::Register`)
    Register(RegisterValidatorCmd),
    /// Deregister a validator (`CValidatorAction::Unregister`)
    Unregister(UnregisterValidatorCmd),
    /// Exit consensus from the consensus signer wallet (`CSignerAction::JailSelf`)
    Jail(SignerJailCmd),
    /// Re-enter consensus from the consensus signer wallet (`CSignerAction::UnjailSelf`)
    Unjail(SignerJailCmd),
    /// Transfer HYPE from spot into staking (`cDeposit`, or `cDeposit2` with `--agent`)
    StakeDeposit(StakeDepositCmd),
    /// Initiate a 7-day HYPE withdrawal from staking into spot (`cWithdraw`)
    StakeWithdraw(StakeWithdrawCmd),
    /// Delegate staked HYPE to a validator (`tokenDelegate`)
    Delegate(DelegateCmd),
    /// Undelegate staked HYPE from a validator (`tokenDelegate`)
    Undelegate(DelegateCmd),
    /// Approve, rotate, or revoke an API/agent wallet (e.g. `aqa-publisher`)
    ApproveAgent(ApproveAgentCmd),
    /// Publish a validator risk-free rate vote for aligned quote assets (`validatorL1Stream`)
    VoteRate(VoteRateCmd),
}

impl ValidatorCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Self::List(cmd) => cmd.run().await,
            Self::Info(cmd) => cmd.run().await,
            Self::RotateSigner(cmd) => cmd.run().await,
            Self::ChangeProfile(cmd) => cmd.run().await,
            Self::Register(cmd) => cmd.run().await,
            Self::Unregister(cmd) => cmd.run().await,
            Self::Jail(cmd) => execute_signer_jail(cmd, true).await,
            Self::Unjail(cmd) => execute_signer_jail(cmd, false).await,
            Self::StakeDeposit(cmd) => cmd.run().await,
            Self::StakeWithdraw(cmd) => cmd.run().await,
            Self::Delegate(cmd) => execute_delegate(cmd, false).await,
            Self::Undelegate(cmd) => execute_delegate(cmd, true).await,
            Self::ApproveAgent(cmd) => cmd.run().await,
            Self::VoteRate(cmd) => cmd.run().await,
        }
    }
}

/// Summary entry returned by `/info` `validatorSummaries`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidatorSummary {
    validator: Address,
    signer: Address,
    name: String,
    description: String,
    n_recent_blocks: u64,
    stake: u64,
    is_jailed: bool,
    unjailable_after: Option<u64>,
    is_active: bool,
    commission: Decimal,
    #[serde(default)]
    stats: Vec<(String, ValidatorPeriodStats)>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidatorPeriodStats {
    uptime_fraction: Decimal,
    predicted_apr: Decimal,
    n_samples: u64,
}

async fn fetch_validator_summaries(client: &HttpClient) -> anyhow::Result<Vec<ValidatorSummary>> {
    let raw = client.validator_summaries().await?;
    serde_json::from_value(raw).context("failed to parse validatorSummaries response")
}

/// List all validators on a chain.
#[derive(Args)]
pub struct ValidatorListCmd {
    /// Only show active validators
    #[arg(long)]
    pub active_only: bool,

    /// Chain to query
    #[arg(long, default_value = "mainnet")]
    pub chain: Chain,
}

impl ValidatorListCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let mut summaries = fetch_validator_summaries(&client).await?;
        if self.active_only {
            summaries.retain(|v| v.is_active);
        }
        summaries.sort_by(|a, b| b.stake.cmp(&a.stake));

        let mut writer = tabwriter::TabWriter::new(stdout());
        let _ = writeln!(
            &mut writer,
            "name\tvalidator\tsigner\tstake (HYPE)\tcommission\tactive\tjailed\tuptime (24h)\tapr (24h)"
        );
        for v in &summaries {
            let day = v.stats.iter().find(|(p, _)| p == "day").map(|(_, s)| s);
            let uptime = day.map_or_else(|| "-".to_string(), |s| format_pct(s.uptime_fraction));
            let apr = day.map_or_else(|| "-".to_string(), |s| format_pct(s.predicted_apr));
            let _ = writeln!(
                &mut writer,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                v.name,
                v.validator,
                v.signer,
                format_staking_wei(v.stake),
                format_pct(v.commission),
                v.is_active,
                v.is_jailed,
                uptime,
                apr,
            );
        }
        let _ = writer.flush();
        Ok(())
    }
}

/// Inspect one validator by address or case-insensitive name.
#[derive(Args)]
pub struct ValidatorInfoCmd {
    /// Validator cold wallet address (`0x...`) or validator name (e.g. `infinitefield.xyz`)
    pub validator: String,

    /// Chain to query
    #[arg(long, default_value = "mainnet")]
    pub chain: Chain,
}

impl ValidatorInfoCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let summaries = fetch_validator_summaries(&client).await?;
        let target_addr = self.validator.parse::<Address>().ok();
        let summary = summaries
            .iter()
            .find(|v| {
                target_addr == Some(v.validator)
                    || v.name.eq_ignore_ascii_case(self.validator.trim())
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "validator '{}' not found on {}. Use 'hypecli validator list' to see validators.",
                    self.validator,
                    self.chain
                )
            })?;

        let addr = summary.validator;
        let (multisig, delegator_summary, delegations, agents) = tokio::join!(
            client.multi_sig_config(addr),
            client.delegator_summary(addr),
            client.delegations(addr),
            client.api_agents(addr),
        );

        let mut writer = tabwriter::TabWriter::new(stdout());
        let _ = writeln!(&mut writer, "name\t{}", summary.name);
        let _ = writeln!(&mut writer, "description\t{}", summary.description);
        let _ = writeln!(&mut writer, "validator\t{}", summary.validator);
        let _ = writeln!(&mut writer, "signer\t{}", summary.signer);
        let _ = writeln!(&mut writer, "active\t{}", summary.is_active);
        let _ = writeln!(&mut writer, "jailed\t{}", summary.is_jailed);
        let _ = writeln!(
            &mut writer,
            "unjailable after\t{}",
            summary
                .unjailable_after
                .map_or_else(|| "-".to_string(), format_timestamp_ms)
        );
        let _ = writeln!(&mut writer, "recent blocks\t{}", summary.n_recent_blocks);
        let _ = writeln!(
            &mut writer,
            "total stake\t{} HYPE",
            format_staking_wei(summary.stake)
        );
        let _ = writeln!(
            &mut writer,
            "commission\t{} ({} bps)",
            format_pct(summary.commission),
            (summary.commission * Decimal::from(10_000u64)).normalize()
        );

        match multisig {
            Ok(cfg) if !cfg.authorized_users.is_empty() => {
                let signers = cfg
                    .authorized_users
                    .iter()
                    .map(Address::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    &mut writer,
                    "multisig\t{}/{} ({signers})",
                    cfg.threshold,
                    cfg.authorized_users.len()
                );
            }
            _ => {
                let _ = writeln!(&mut writer, "multisig\tnone");
            }
        }

        if let Ok(ds) = delegator_summary {
            let _ = writeln!(&mut writer);
            let _ = writeln!(&mut writer, "self delegated\t{} HYPE", ds.delegated);
            let _ = writeln!(&mut writer, "undelegated stake\t{} HYPE", ds.undelegated);
            let _ = writeln!(
                &mut writer,
                "pending withdrawal\t{} HYPE ({} queued)",
                ds.total_pending_withdrawal, ds.n_pending_withdrawals
            );
        }

        if !summary.stats.is_empty() {
            let _ = writeln!(&mut writer);
            let _ = writeln!(&mut writer, "period\tuptime\tpredicted apr\tsamples");
            for (period, stats) in &summary.stats {
                let _ = writeln!(
                    &mut writer,
                    "{period}\t{}\t{}\t{}",
                    format_pct(stats.uptime_fraction),
                    format_pct(stats.predicted_apr),
                    stats.n_samples
                );
            }
        }

        if let Ok(delegations) = delegations
            && !delegations.is_empty()
        {
            let _ = writeln!(&mut writer);
            let _ = writeln!(&mut writer, "delegated to\tamount (HYPE)\tlocked until");
            for d in delegations {
                let locked = d
                    .locked_until_timestamp
                    .map_or_else(|| "-".to_string(), format_timestamp_ms);
                let _ = writeln!(&mut writer, "{}\t{}\t{locked}", d.validator, d.amount);
            }
        }

        if let Ok(agents) = agents
            && !agents.is_empty()
        {
            let _ = writeln!(&mut writer);
            let _ = writeln!(&mut writer, "agent name\taddress\tvalid until");
            for a in agents {
                let until = a
                    .valid_until
                    .map_or_else(|| "-".to_string(), format_timestamp_ms);
                let _ = writeln!(&mut writer, "{}\t{}\t{until}", a.name, a.address);
            }
        }

        let _ = writer.flush();
        Ok(())
    }
}

/// Rotate the validator's consensus signer address (`CValidatorAction::ChangeProfile`).
#[derive(Args, derive_more::Deref)]
pub struct RotateSignerCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// New consensus signer hot-wallet address
    #[arg(long, visible_alias = "signer")]
    pub new_signer: Address,

    /// Explicitly mark the validator unjailed (`true`). Defaults to preserving current status.
    #[arg(long, conflicts_with = "jail")]
    pub unjailed: bool,

    /// Explicitly jail the validator (`unjailed = false`) during rotation.
    #[arg(long, conflicts_with = "unjailed")]
    pub jail: bool,
}

impl RotateSignerCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        ensure!(
            self.new_signer != Address::ZERO,
            "consensus signer address cannot be the zero address"
        );
        let client = HttpClient::new(self.signer.chain);
        let summaries = if !self.unjailed && !self.jail {
            Some(fetch_validator_summaries(&client).await?)
        } else {
            None
        };
        let explicit = explicit_unjailed(self.unjailed, self.jail);
        let new_signer = self.new_signer;
        let chain = self.signer.chain;

        self.signer
            .try_execute_default(client, |account, _| {
                let unjailed = resolve_unjailed(explicit, summaries.as_deref(), account, chain)?;
                println!(
                    "Rotating consensus signer for validator {account} to {new_signer} (unjailed={unjailed})"
                );
                Ok(build_rotate_signer(new_signer, unjailed))
            })
            .await?;
        println!("Rotated consensus signer successfully.");
        Ok(())
    }
}

/// Change a validator's profile (`CValidatorAction::ChangeProfile`).
#[derive(Args, derive_more::Deref)]
pub struct ChangeProfileCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// Consensus node IP address (note: changing IP automatically jails the validator on L1)
    #[arg(long)]
    pub ip: Option<IpAddr>,

    /// Public validator display name
    #[arg(long)]
    pub name: Option<String>,

    /// Public validator description
    #[arg(long)]
    pub description: Option<String>,

    /// Commission rate in basis points (`0..=10000`; cannot increase above 100 bps = 1%)
    #[arg(long)]
    pub commission_bps: Option<u64>,

    /// Disable new delegations to this validator
    #[arg(long, conflicts_with = "enable_delegations")]
    pub disable_delegations: bool,

    /// Allow delegations to this validator
    #[arg(long, conflicts_with = "disable_delegations")]
    pub enable_delegations: bool,

    /// New consensus signer hot-wallet address
    #[arg(long, visible_alias = "signer")]
    pub new_signer: Option<Address>,

    /// Explicitly set `unjailed = true`. Defaults to preserving current status.
    #[arg(long, conflicts_with = "jail")]
    pub unjailed: bool,

    /// Explicitly set `unjailed = false`.
    #[arg(long, conflicts_with = "unjailed")]
    pub jail: bool,
}

impl ChangeProfileCmd {
    fn validate(&self) -> anyhow::Result<Option<bool>> {
        if let Some(bps) = self.commission_bps {
            ensure!(
                bps <= MAX_COMMISSION_BPS,
                "--commission-bps must be at most {MAX_COMMISSION_BPS} (100%)"
            );
        }
        if let Some(addr) = self.new_signer {
            ensure!(
                addr != Address::ZERO,
                "consensus signer address cannot be the zero address"
            );
        }
        let disable_delegations = if self.disable_delegations {
            Some(true)
        } else if self.enable_delegations {
            Some(false)
        } else {
            None
        };
        let has_change = self.ip.is_some()
            || self.name.is_some()
            || self.description.is_some()
            || self.commission_bps.is_some()
            || disable_delegations.is_some()
            || self.new_signer.is_some()
            || self.unjailed
            || self.jail;
        ensure!(
            has_change,
            "specify at least one profile field to change (--ip, --name, --description, --commission-bps, --disable-delegations, --enable-delegations, --new-signer, --unjailed, or --jail)"
        );
        Ok(disable_delegations)
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let disable_delegations = self.validate()?;
        let client = HttpClient::new(self.signer.chain);
        let explicit = explicit_unjailed(self.unjailed, self.jail);
        let summaries = if explicit.is_none() {
            Some(fetch_validator_summaries(&client).await?)
        } else {
            None
        };
        let chain = self.signer.chain;
        let ip = self.ip;
        let name = self.name;
        let description = self.description;
        let commission_bps = self.commission_bps;
        let new_signer = self.new_signer;

        self.signer
            .try_execute_default(client, |account, _| {
                let unjailed = resolve_unjailed(explicit, summaries.as_deref(), account, chain)?;
                println!("Updating validator profile for {account} (unjailed={unjailed})");
                Ok(Action::CValidatorAction(CValidatorAction::ChangeProfile(
                    ValidatorProfileChange {
                        node_ip: ip.map(|ip| ValidatorNodeIp { ip: ip.to_string() }),
                        name,
                        description,
                        unjailed,
                        disable_delegations,
                        commission_bps,
                        signer: new_signer,
                    },
                )))
            })
            .await?;
        println!("Updated validator profile successfully.");
        Ok(())
    }
}

/// Register a new validator (`CValidatorAction::Register`).
#[derive(Args, derive_more::Deref)]
pub struct RegisterValidatorCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// Consensus node IP address
    #[arg(long)]
    pub ip: IpAddr,

    /// Public validator display name
    #[arg(long)]
    pub name: String,

    /// Public validator description
    #[arg(long)]
    pub description: String,

    /// Commission rate in basis points (`0..=10000`, e.g. `100` = 1%)
    #[arg(long)]
    pub commission_bps: u64,

    /// Consensus signer hot-wallet address
    #[arg(long, visible_alias = "signer")]
    pub consensus_signer: Address,

    /// Initial self-delegated stake in HYPE (mainnet requires at least 10,000 HYPE)
    #[arg(long)]
    pub initial_stake: Decimal,

    /// Refuse delegations from external stakers
    #[arg(long)]
    pub disable_delegations: bool,

    /// Start unjailed instead of jailed (new validators normally start jailed)
    #[arg(long)]
    pub unjailed: bool,
}

impl RegisterValidatorCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        ensure!(
            self.commission_bps <= MAX_COMMISSION_BPS,
            "--commission-bps must be at most {MAX_COMMISSION_BPS} (100%)"
        );
        ensure!(
            self.consensus_signer != Address::ZERO,
            "consensus signer address cannot be the zero address"
        );
        let initial_wei = to_staking_wei(self.initial_stake)?;
        let client = HttpClient::new(self.signer.chain);
        let ip = self.ip;
        let name = self.name;
        let description = self.description;
        let delegations_disabled = self.disable_delegations;
        let commission_bps = self.commission_bps;
        let consensus_signer = self.consensus_signer;
        let unjailed = self.unjailed;
        let initial_stake = self.initial_stake;

        self.signer
            .execute_default(client, |account, _| {
                println!(
                    "Registering validator {account} with signer {consensus_signer} and initial stake {initial_stake} HYPE"
                );
                Action::CValidatorAction(CValidatorAction::Register(ValidatorRegistration {
                    profile: ValidatorProfile {
                        node_ip: ValidatorNodeIp { ip: ip.to_string() },
                        name,
                        description,
                        delegations_disabled,
                        commission_bps,
                        signer: consensus_signer,
                    },
                    unjailed,
                    initial_wei,
                }))
            })
            .await?;
        println!("Registered validator successfully.");
        Ok(())
    }
}

/// Deregister an existing validator (`CValidatorAction::Unregister`).
#[derive(Args, derive_more::Deref)]
pub struct UnregisterValidatorCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,
}

impl UnregisterValidatorCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.signer.chain);
        self.signer
            .execute_default(client, |account, _| {
                println!("Unregistering validator {account}");
                Action::CValidatorAction(CValidatorAction::Unregister(()))
            })
            .await?;
        println!("Unregistered validator successfully.");
        Ok(())
    }
}

/// Jail or unjail from the consensus signer wallet (`CSignerAction`).
#[derive(Args, derive_more::Deref)]
pub struct SignerJailCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,
}

async fn execute_signer_jail(cmd: SignerJailCmd, jail: bool) -> anyhow::Result<()> {
    let (verb, past, action) = if jail {
        ("Jailing", "Jailed", CSignerAction::JailSelf(()))
    } else {
        ("Unjailing", "Unjailed", CSignerAction::UnjailSelf(()))
    };
    let client = HttpClient::new(cmd.signer.chain);
    cmd.signer
        .execute_default(client, |account, _| {
            println!("{verb} validator via consensus signer {account}");
            Action::CSignerAction(action)
        })
        .await?;
    println!("{past} successfully.");
    Ok(())
}

/// Deposit HYPE from spot into the staking balance (`cDeposit` or `cDeposit2`).
#[derive(Args, derive_more::Deref)]
pub struct StakeDepositCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// Amount of HYPE to deposit into staking (`1 HYPE = 10^8 wei`)
    #[arg(long)]
    pub amount: Decimal,

    /// Use the agent-signed L1 `cDeposit2` action instead of user-signed `cDeposit`
    #[arg(long)]
    pub agent: bool,
}

impl StakeDepositCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let wei = to_staking_wei(self.amount)?;
        let chain = self.signer.chain;
        let client = HttpClient::new(chain);
        let amount = self.amount;
        let agent = self.agent;

        self.signer
            .execute_default(client, |account, nonce| {
                println!("Depositing {amount} HYPE from spot into staking for {account}");
                if agent {
                    Action::CDeposit2 { wei }
                } else {
                    Action::CDeposit(CDepositAction {
                        signature_chain_id: chain.arbitrum_id().to_owned(),
                        hyperliquid_chain: chain,
                        wei,
                        nonce,
                    })
                }
            })
            .await?;
        println!("Deposited into staking successfully.");
        Ok(())
    }
}

/// Withdraw HYPE from the staking balance into spot (`cWithdraw`, 7-day unstaking queue).
#[derive(Args, derive_more::Deref)]
pub struct StakeWithdrawCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// Amount of HYPE to withdraw from staking (`1 HYPE = 10^8 wei`)
    #[arg(long)]
    pub amount: Decimal,
}

impl StakeWithdrawCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let wei = to_staking_wei(self.amount)?;
        let chain = self.signer.chain;
        let client = HttpClient::new(chain);
        let amount = self.amount;

        self.signer
            .execute_default(client, |account, nonce| {
                println!("Initiating staking withdrawal of {amount} HYPE for {account}");
                Action::CWithdraw(CWithdrawAction {
                    signature_chain_id: chain.arbitrum_id().to_owned(),
                    hyperliquid_chain: chain,
                    wei,
                    nonce,
                })
            })
            .await?;
        println!("Initiated staking withdrawal successfully.");
        Ok(())
    }
}

/// Delegate or undelegate staked HYPE (`tokenDelegate`).
#[derive(Args, derive_more::Deref)]
pub struct DelegateCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// Target validator cold-wallet address
    #[arg(long)]
    pub validator: Address,

    /// Amount of HYPE to delegate or undelegate (`1 HYPE = 10^8 wei`)
    #[arg(long)]
    pub amount: Decimal,
}

async fn execute_delegate(cmd: DelegateCmd, is_undelegate: bool) -> anyhow::Result<()> {
    let wei = to_staking_wei(cmd.amount)?;
    let (verb, prep, past) = if is_undelegate {
        ("Undelegating", "from", "Undelegated")
    } else {
        ("Delegating", "to", "Delegated")
    };
    let chain = cmd.signer.chain;
    let client = HttpClient::new(chain);
    let validator = cmd.validator;
    let amount = cmd.amount;

    cmd.signer
        .execute_default(client, |account, nonce| {
            println!("{verb} {amount} HYPE {prep} validator {validator} for {account}");
            Action::TokenDelegate(TokenDelegateAction {
                signature_chain_id: chain.arbitrum_id().to_owned(),
                hyperliquid_chain: chain,
                validator,
                wei,
                is_undelegate,
                nonce,
            })
        })
        .await?;
    println!("{past} successfully.");
    Ok(())
}

/// Approve, rotate, or revoke an API/agent wallet (`approveAgent`).
#[derive(Args, derive_more::Deref)]
pub struct ApproveAgentCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// Agent wallet address (`0x0000000000000000000000000000000000000000` revokes)
    #[arg(long)]
    pub agent: Address,

    /// Optional agent name (e.g. `publisher`; append ` valid_until <ms>` for expiry)
    #[arg(long)]
    pub name: Option<String>,
}

impl ApproveAgentCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let chain = self.signer.chain;
        let client = HttpClient::new(chain);
        let agent_address = self.agent;
        let agent_name = self.name.filter(|s| !s.is_empty());

        self.signer
            .execute_default(client, |account, nonce| {
                match &agent_name {
                    Some(name) => println!(
                        "Approving agent '{name}' ({agent_address}) for account {account}"
                    ),
                    None => {
                        println!("Approving unnamed agent ({agent_address}) for account {account}")
                    }
                }
                Action::ApproveAgent(ApproveAgent {
                    signature_chain_id: chain.arbitrum_id().to_owned(),
                    hyperliquid_chain: chain,
                    agent_address,
                    agent_name,
                    nonce,
                })
            })
            .await?;
        println!("Approved agent successfully.");
        Ok(())
    }
}

/// Submit a validator aligned-quote-asset risk-free rate vote (`validatorL1Stream`).
#[derive(Args, derive_more::Deref)]
pub struct VoteRateCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// Annualized risk-free rate as a decimal fraction (e.g. `0.04` for 4%)
    #[arg(long)]
    pub rate: Decimal,
}

impl VoteRateCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        ensure!(
            !self.rate.is_sign_negative(),
            "--rate cannot be negative: {}",
            self.rate
        );
        let client = HttpClient::new(self.signer.chain);
        let risk_free_rate = self.rate;

        self.signer
            .execute_default(client, |account, _| {
                println!(
                    "Submitting validatorL1Stream risk-free rate {} ({}) from {account}",
                    risk_free_rate,
                    format_pct(risk_free_rate)
                );
                Action::ValidatorL1Stream(ValidatorL1Stream { risk_free_rate })
            })
            .await?;
        println!("Submitted risk-free rate vote successfully.");
        Ok(())
    }
}

fn build_rotate_signer(new_signer: Address, unjailed: bool) -> Action {
    Action::CValidatorAction(CValidatorAction::ChangeProfile(ValidatorProfileChange {
        node_ip: None,
        name: None,
        description: None,
        unjailed,
        disable_delegations: None,
        commission_bps: None,
        signer: Some(new_signer),
    }))
}

fn explicit_unjailed(unjailed: bool, jail: bool) -> Option<bool> {
    if unjailed {
        Some(true)
    } else if jail {
        Some(false)
    } else {
        None
    }
}

fn resolve_unjailed(
    explicit: Option<bool>,
    summaries: Option<&[ValidatorSummary]>,
    account: Address,
    chain: Chain,
) -> anyhow::Result<bool> {
    if let Some(flag) = explicit {
        return Ok(flag);
    }
    let summaries = summaries.context("validator summaries missing for automatic unjailed check")?;
    let Some(summary) = summaries.iter().find(|v| v.validator == account) else {
        bail!(
            "account {account} is not a registered validator on {chain}; pass --multi-sig-addr <VALIDATOR_ADDR> or set --unjailed / --jail explicitly"
        );
    };
    Ok(!summary.is_jailed)
}

/// Convert a positive HYPE decimal amount to 8-decimal staking wei (`u64`) without truncation.
fn to_staking_wei(amount: Decimal) -> anyhow::Result<u64> {
    let normalized = amount.normalize();
    ensure!(
        normalized > Decimal::ZERO,
        "staking amount must be positive: {amount}"
    );
    ensure!(
        normalized.scale() <= STAKING_WEI_DECIMALS,
        "staking amount {amount} exceeds {STAKING_WEI_DECIMALS} decimal places (1 HYPE = 10^8 wei)"
    );
    u64::try_from(to_wei(normalized, STAKING_WEI_DECIMALS))
        .map_err(|_| anyhow::anyhow!("staking amount {amount} HYPE overflows u64 wei"))
}

fn format_staking_wei(wei: u64) -> Decimal {
    from_wei(U256::from(wei), STAKING_WEI_DECIMALS).normalize()
}

fn format_pct(fraction: Decimal) -> String {
    format!("{}%", (fraction * Decimal::ONE_HUNDRED).round_dp(2))
}

fn format_timestamp_ms(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| ms.to_string())
}

#[cfg(test)]
mod tests {
    use alloy::{
        dyn_abi::TypedData,
        primitives::{Address, B256, ChainId, address},
        signers::{Signer, local::PrivateKeySigner},
    };
    use hypersdk::{
        dec,
        hypercore::{
            Chain,
            api::{Action, MultiSigPayload},
            signing::{multisig_collect_signatures, multisig_lead_msg},
        },
    };

    use super::*;

    struct TypedDataOnlySigner(PrivateKeySigner);

    #[async_trait::async_trait]
    impl Signer for TypedDataOnlySigner {
        async fn sign_hash(&self, _: &B256) -> alloy::signers::Result<alloy::signers::Signature> {
            Err(alloy::signers::Error::UnsupportedOperation(
                alloy::signers::UnsupportedSignerOperation::SignHash,
            ))
        }

        async fn sign_dynamic_typed_data(
            &self,
            data: &TypedData,
        ) -> alloy::signers::Result<alloy::signers::Signature> {
            self.0.sign_dynamic_typed_data(data).await
        }

        fn address(&self) -> Address {
            self.0.address()
        }

        fn chain_id(&self) -> Option<ChainId> {
            self.0.chain_id()
        }

        fn set_chain_id(&mut self, chain_id: Option<ChainId>) {
            self.0.set_chain_id(chain_id);
        }
    }

    #[test]
    fn staking_wei_uses_eight_decimals_and_rejects_truncation() {
        assert_eq!(to_staking_wei(dec!(10_000)).unwrap(), 1_000_000_000_000);
        assert_eq!(to_staking_wei(dec!(1)).unwrap(), 100_000_000);
        assert_eq!(to_staking_wei(dec!(0.12345678)).unwrap(), 12_345_678);
        assert_eq!(
            format_staking_wei(1_727_734_843_260_687),
            dec!(17277348.43260687)
        );
        assert!(to_staking_wei(Decimal::ZERO).is_err());
        assert!(to_staking_wei(dec!(-1)).is_err());
        assert!(to_staking_wei(dec!(0.000000001)).is_err());
    }

    #[test]
    fn resolve_unjailed_preserves_live_validator_status_unless_overridden() {
        let active_addr = address!("0xa23b4556090260828ff3f939d2dbdd4f318b5f1f");
        let jailed_addr = address!("0x1111111111111111111111111111111111111111");
        let unknown = address!("0x2222222222222222222222222222222222222222");
        let summaries = vec![
            ValidatorSummary {
                validator: active_addr,
                signer: unknown,
                name: "infinitefield.xyz".into(),
                description: "desc".into(),
                n_recent_blocks: 4,
                stake: 1_000_000_000_000,
                is_jailed: false,
                unjailable_after: None,
                is_active: true,
                commission: Decimal::ZERO,
                stats: vec![],
            },
            ValidatorSummary {
                validator: jailed_addr,
                signer: unknown,
                name: "jailed".into(),
                description: "desc".into(),
                n_recent_blocks: 0,
                stake: 1_000_000_000_000,
                is_jailed: true,
                unjailable_after: Some(1_800_000_000_000),
                is_active: false,
                commission: Decimal::ZERO,
                stats: vec![],
            },
        ];

        assert!(resolve_unjailed(None, Some(&summaries), active_addr, Chain::Mainnet).unwrap());
        assert!(!resolve_unjailed(None, Some(&summaries), jailed_addr, Chain::Mainnet).unwrap());
        assert!(!resolve_unjailed(Some(false), None, active_addr, Chain::Mainnet).unwrap());
        assert!(resolve_unjailed(Some(true), None, jailed_addr, Chain::Mainnet).unwrap());
        assert!(resolve_unjailed(None, Some(&summaries), unknown, Chain::Mainnet).is_err());
    }

    #[test]
    fn rotate_signer_serializes_expected_change_profile_payload() {
        let new_signer = address!("0xe02dc4639abce8f93adb7f36244a309f5aacada5");
        let action = build_rotate_signer(new_signer, true);
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            serde_json::json!({
                "type": "CValidatorAction",
                "changeProfile": {
                    "node_ip": null,
                    "name": null,
                    "description": null,
                    "unjailed": true,
                    "disable_delegations": null,
                    "commission_bps": null,
                    "signer": "0xe02dc4639abce8f93adb7f36244a309f5aacada5"
                }
            })
        );
    }

    #[tokio::test]
    async fn validator_actions_roundtrip_p2p_msgpack_and_sign_multisig_on_trezor() {
        let key1 = PrivateKeySigner::random();
        let key2 = PrivateKeySigner::random();
        let signer1: Box<dyn Signer + Send + Sync> = Box::new(TypedDataOnlySigner(key1.clone()));
        let signer2: Box<dyn Signer + Send + Sync> = Box::new(TypedDataOnlySigner(key2.clone()));
        let multisig = address!("0xa23b4556090260828ff3f939d2dbdd4f318b5f1f");
        let hot = address!("0xe02dc4639abce8f93adb7f36244a309f5aacada5");
        let nonce = 1_760_000_000_000u64;

        for chain in [Chain::Mainnet, Chain::Testnet] {
            let sig_chain = chain.arbitrum_id().to_owned();
            let actions = vec![
                build_rotate_signer(hot, true),
                Action::CValidatorAction(CValidatorAction::ChangeProfile(ValidatorProfileChange {
                    node_ip: Some(ValidatorNodeIp {
                        ip: "52.68.71.160".into(),
                    }),
                    name: Some("infinitefield.xyz".into()),
                    description: Some("HFT market making infrastructure".into()),
                    unjailed: false,
                    disable_delegations: Some(false),
                    commission_bps: Some(100),
                    signer: None,
                })),
                Action::CValidatorAction(CValidatorAction::Register(ValidatorRegistration {
                    profile: ValidatorProfile {
                        node_ip: ValidatorNodeIp {
                            ip: "52.68.71.160".into(),
                        },
                        name: "infinitefield.xyz".into(),
                        description: "HFT market making infrastructure".into(),
                        delegations_disabled: false,
                        commission_bps: 0,
                        signer: hot,
                    },
                    unjailed: false,
                    initial_wei: 1_000_000_000_000,
                })),
                Action::CValidatorAction(CValidatorAction::Unregister(())),
                Action::CSignerAction(CSignerAction::JailSelf(())),
                Action::CSignerAction(CSignerAction::UnjailSelf(())),
                Action::CDeposit(CDepositAction {
                    signature_chain_id: sig_chain.clone(),
                    hyperliquid_chain: chain,
                    wei: 100_000_000,
                    nonce,
                }),
                Action::CDeposit2 { wei: 100_000_000 },
                Action::CWithdraw(CWithdrawAction {
                    signature_chain_id: sig_chain.clone(),
                    hyperliquid_chain: chain,
                    wei: 100_000_000,
                    nonce,
                }),
                Action::TokenDelegate(TokenDelegateAction {
                    signature_chain_id: sig_chain.clone(),
                    hyperliquid_chain: chain,
                    validator: multisig,
                    wei: 1_000_000_000_000,
                    is_undelegate: false,
                    nonce,
                }),
                Action::ApproveAgent(ApproveAgent {
                    signature_chain_id: sig_chain.clone(),
                    hyperliquid_chain: chain,
                    agent_address: hot,
                    agent_name: Some("publisher".into()),
                    nonce,
                }),
                Action::ApproveAgent(ApproveAgent {
                    signature_chain_id: sig_chain,
                    hyperliquid_chain: chain,
                    agent_address: Address::ZERO,
                    agent_name: None,
                    nonce,
                }),
                Action::ValidatorL1Stream(ValidatorL1Stream {
                    risk_free_rate: dec!(0.04),
                }),
            ];

            for action in actions {
                // Single-signer hardware wallet signing + recovery.
                let single = action
                    .clone()
                    .sign(&signer1, nonce, None, None, chain)
                    .await
                    .unwrap();
                assert_eq!(single.recover(chain).unwrap(), key1.address());

                // P2P gossip msgpack round-trip (`rmp_serde::to_vec` -> `from_slice`),
                // matching `multisig::proto::Codec`.
                let proposal = MultiSigPayload {
                    multi_sig_user: multisig.to_string().to_lowercase(),
                    outer_signer: key1.address().to_string().to_lowercase(),
                    action: Box::new(action.clone()),
                };
                let wire = rmp_serde::to_vec(&(nonce, &proposal)).unwrap();
                let (decoded_nonce, decoded_proposal): (u64, MultiSigPayload) =
                    rmp_serde::from_slice(&wire).unwrap();
                assert_eq!(decoded_nonce, nonce);
                assert_eq!(
                    proposal.prehash(nonce, chain).unwrap(),
                    decoded_proposal.prehash(nonce, chain).unwrap(),
                    "msgpack round-trip changed multisig prehash for {action:?}"
                );

                // Co-signer signs the decoded P2P proposal; leader verifies & wraps in SendMultiSig.
                let remote_sig = decoded_proposal.sign(&signer2, nonce, chain).await.unwrap();
                assert_eq!(
                    proposal.recover(&remote_sig, nonce, chain).unwrap(),
                    key2.address()
                );
                let multi = multisig_collect_signatures(
                    key1.address(),
                    multisig,
                    std::iter::once(&signer1),
                    std::iter::once(remote_sig),
                    action,
                    nonce,
                    chain,
                )
                .await
                .unwrap();
                assert_eq!(multi.signatures.len(), 2);
                let outer = multisig_lead_msg(&signer1, multi, nonce, None, None, chain)
                    .await
                    .unwrap();
                assert_eq!(outer.recover(chain).unwrap(), key1.address());
            }
        }
    }
}
