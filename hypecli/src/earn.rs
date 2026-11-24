//! Hyperliquid Earn (borrow/lend reserve) commands.
//!
//! Earn is the borrow/lend market: suppliers lend a quote asset (USDC) to
//! borrowers and earn the borrow interest. On the wire it is the undocumented
//! `borrowLend` action with operations `supply` and `withdraw`.
//!
//! Token index `0` is USDC.

use alloy::primitives::Address;
use clap::{Args, Subcommand};
use hypersdk::Decimal;
use hypersdk::hypercore::api::{Action, BorrowLendAction, BorrowLendOperation};
use hypersdk::hypercore::{Chain, HttpClient};
use serde::Deserialize;

use crate::action::ActionArgs;
use crate::utils::resolve_token;

/// Earn supply and withdrawal commands.
#[derive(Subcommand)]
pub enum EarnCmd {
    /// Supply tokens into the Earn reserve to earn interest
    Supply(EarnTransferCmd),
    /// Withdraw supplied tokens from the Earn reserve
    Withdraw(EarnTransferCmd),
    /// Query reserve rates and a user's position
    Status(EarnStatusCmd),
}

impl EarnCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            EarnCmd::Supply(cmd) => execute_transfer(cmd, BorrowLendOperation::Supply).await,
            EarnCmd::Withdraw(cmd) => execute_transfer(cmd, BorrowLendOperation::Withdraw).await,
            EarnCmd::Status(cmd) => cmd.run().await,
        }
    }
}

async fn execute_transfer(
    cmd: EarnTransferCmd,
    operation: BorrowLendOperation,
) -> anyhow::Result<()> {
    let (verb, past) = match operation {
        BorrowLendOperation::Supply => ("Supplying", "Supplied"),
        BorrowLendOperation::Withdraw => ("Withdrawing", "Withdrawn"),
        _ => unreachable!("earn only supplies or withdraws"),
    };

    let client = HttpClient::new(cmd.signer.chain);
    let tokens = client.spot_tokens().await?;
    let token = resolve_token(&tokens, &cmd.token)?;
    let amount = cmd
        .amount
        .map(|a| a.to_string())
        .unwrap_or_else(|| "max".to_string());

    println!("{} {} {} ...", verb, amount, token.name);
    let action = BorrowLendAction {
        operation,
        token: token.index,
        amount: cmd.amount,
    };
    cmd.signer
        .execute_default(client, |_, _| Action::BorrowLend(action))
        .await?;
    println!("{} successfully.", past);
    Ok(())
}

/// Arguments for an Earn supply or withdrawal.
#[derive(Args, derive_more::Deref)]
pub struct EarnTransferCmd {
    #[deref]
    #[command(flatten)]
    pub signer: ActionArgs,

    /// Amount to supply or withdraw. Omit for the maximum available.
    #[arg(long)]
    pub amount: Option<Decimal>,

    /// Reserve token symbol or index (e.g. USDC, USDT0, or 0)
    #[arg(long, default_value = "USDC", value_name = "SYMBOL_OR_INDEX")]
    pub token: String,
}

/// Arguments for the Earn status query.
#[derive(Args)]
pub struct EarnStatusCmd {
    /// User address to query the position for
    #[arg(long)]
    pub user: Address,

    /// Reserve token symbol or index (e.g. USDC, USDT0, or 0)
    #[arg(long, default_value = "USDC", value_name = "SYMBOL_OR_INDEX")]
    pub token: String,

    /// Target chain for the query
    #[arg(long, default_value = "mainnet")]
    pub chain: Chain,
}

impl EarnStatusCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let tokens = client.spot_tokens().await?;
        let token = resolve_token(&tokens, &self.token)?;
        let name = &token.name;

        let reserve: ReserveState =
            serde_json::from_value(client.borrow_lend_reserve_state(token.index).await?)?;
        let user: UserState =
            serde_json::from_value(client.borrow_lend_user_state(self.user).await?)?;

        let pct = |d: &str| format_pct(d);
        println!("Reserve: {} (token {})", name, token.index);
        println!("Supply APY: {}%", pct(&reserve.supply_yearly_rate));
        println!("Borrow APY: {}%", pct(&reserve.borrow_yearly_rate));
        println!("Utilization: {}%", pct(&reserve.utilization));
        println!("Total Supplied: {} {}", reserve.total_supplied, name);
        println!("Total Borrowed: {} {}", reserve.total_borrowed, name);
        println!("Oracle Price: ${}", reserve.oracle_px);

        let state = user
            .token_to_state
            .iter()
            .find(|(index, _)| *index == token.index)
            .map(|(_, state)| state);
        let supplied = state.and_then(|s| s.supply.as_ref());
        let borrowed = state.and_then(|s| s.borrow.as_ref());

        println!();
        println!("Your Position:");
        println!(
            "  Supplied: {} {}",
            supplied.map(|s| s.value.as_str()).unwrap_or("0"),
            name
        );
        println!(
            "  Borrowed: {} {}",
            borrowed.map(|b| b.value.as_str()).unwrap_or("0"),
            name
        );
        println!("  Health: {}", user.health);
        Ok(())
    }
}

/// Reserve-level rate and liquidity state.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReserveState {
    borrow_yearly_rate: String,
    supply_yearly_rate: String,
    total_supplied: String,
    total_borrowed: String,
    utilization: String,
    oracle_px: String,
}

/// A user's aggregate borrow/lend state.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserState {
    token_to_state: Vec<(u32, TokenState)>,
    health: String,
}

#[derive(Deserialize)]
struct TokenState {
    #[serde(default)]
    supply: Option<Side>,
    #[serde(default)]
    borrow: Option<Side>,
}

#[derive(Deserialize)]
struct Side {
    value: String,
}

/// Render a fractional decimal string as a percentage with two places.
fn format_pct(value: &str) -> String {
    value
        .parse::<Decimal>()
        .map(|d| (d * Decimal::ONE_HUNDRED).round_dp(2))
        .map(|d| d.to_string())
        .unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod pct_tests {
    use super::format_pct;

    #[test]
    fn renders_fraction_as_percent() {
        assert_eq!(format_pct("0.0290714004"), "2.91");
        assert_eq!(format_pct("0.6460311198"), "64.60");
        assert_eq!(format_pct("garbage"), "garbage");
    }
}

#[cfg(test)]
mod tests {
    use alloy::{
        dyn_abi::TypedData,
        primitives::{Address, B256, ChainId},
        signers::{Signer, local::PrivateKeySigner},
    };
    use clap::Parser;
    use hypersdk::hypercore::api::{Action, BorrowLendAction, BorrowLendOperation};
    use hypersdk::hypercore::{Chain, api::MultiSigPayload};
    use rust_decimal::dec;

    use super::EarnCmd;
    use crate::{Cli, Command};

    // Match Trezor's capabilities, including dispatch through a boxed signer.
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

    #[tokio::test]
    async fn user_signed_actions_support_boxed_typed_data_only_signers() {
        let signer: Box<dyn Signer + Send + Sync> =
            Box::new(TypedDataOnlySigner(PrivateKeySigner::random()));
        use serde_json::json;
        let address = "0x2222222222222222222222222222222222222222";
        let cases = [
            json!({"type":"approveAgent", "agentAddress":address,"agentName":null}),
            json!({"type":"approveBuilderFee", "builder":address,"maxFeeRate":"0.001%"}),
            json!({"type":"cDeposit", "wei":1000}),
            json!({"type":"cWithdraw", "wei":1000}),
            json!({"type":"tokenDelegate", "validator":address,"wei":1000,"isUndelegate":false}),
            json!({"type":"withdraw3", "destination":address,"amount":"10","time":1}),
        ];
        for mut case in cases {
            let chain = Chain::Mainnet;
            let nonce = 1;
            case["signatureChainId"] = "0xa4b1".into();
            case["hyperliquidChain"] = "Mainnet".into();
            case["nonce"] = nonce.into();
            let action: Action = serde_json::from_value(case).unwrap();
            let ordinary = action
                .clone()
                .sign(&signer, nonce, None, None, chain)
                .await
                .unwrap();
            assert_eq!(ordinary.recover(chain).unwrap(), signer.address());
            let payload = MultiSigPayload {
                multi_sig_user: "0x1111111111111111111111111111111111111111".into(),
                outer_signer: signer.address().to_string().to_lowercase(),
                action: Box::new(action),
            };
            let sig = payload.sign(&signer, nonce, chain).await.unwrap();
            assert_eq!(
                payload.recover(&sig, nonce, chain).unwrap(),
                signer.address()
            );
        }
    }

    #[tokio::test]
    async fn earn_supports_boxed_typed_data_only_signers() {
        use hypersdk::hypercore::signing::{multisig_collect_signatures, multisig_lead_msg};

        let key = PrivateKeySigner::random();
        let signer: Box<dyn Signer + Send + Sync> = Box::new(TypedDataOnlySigner(key.clone()));
        let account = "0x1111111111111111111111111111111111111111"
            .parse()
            .unwrap();
        let nonce = 1_700_000_000_000;
        for chain in [Chain::Mainnet, Chain::Testnet] {
            for operation in [BorrowLendOperation::Supply, BorrowLendOperation::Withdraw] {
                for amount in [Some(dec!(100)), None] {
                    let action = Action::BorrowLend(BorrowLendAction {
                        operation,
                        token: 0,
                        amount,
                    });
                    let expected = action
                        .clone()
                        .sign_sync(&key, nonce, None, None, chain)
                        .unwrap();
                    let actual = action
                        .clone()
                        .sign(&signer, nonce, None, None, chain)
                        .await
                        .unwrap();
                    assert_eq!(actual.signature.to_string(), expected.signature.to_string());

                    let multisig = multisig_collect_signatures(
                        signer.address(),
                        account,
                        std::iter::once(&signer),
                        std::iter::empty(),
                        action,
                        nonce,
                        chain,
                    )
                    .await
                    .unwrap();
                    let expected = multisig.payload.sign_sync(&key, nonce, chain).unwrap();
                    let actual = multisig.payload.sign(&signer, nonce, chain).await.unwrap();
                    assert_eq!(actual.to_string(), expected.to_string());
                    assert_eq!(multisig.signatures[0].to_string(), expected.to_string());
                    let request = multisig_lead_msg(&signer, multisig, nonce, None, None, chain)
                        .await
                        .unwrap();
                    assert_eq!(
                        request
                            .action
                            .recover(&request.signature, nonce, None, None, chain)
                            .unwrap(),
                        key.address()
                    );
                }
            }
        }
    }

    #[test]
    fn all_earn_commands_accept_token_symbols_and_indexes() {
        for operation in ["supply", "withdraw", "status"] {
            for selector in ["USDC", "USDT0", "usdt0", "268"] {
                let mut args = vec!["hypecli", "earn", operation, "--token", selector];
                if operation == "status" {
                    args.extend(["--user", "0x1111111111111111111111111111111111111111"]);
                }
                let cli = Cli::try_parse_from(args).unwrap();
                let parsed = match cli.command.unwrap() {
                    Command::Earn(EarnCmd::Supply(cmd) | EarnCmd::Withdraw(cmd)) => cmd.token,
                    Command::Earn(EarnCmd::Status(cmd)) => cmd.token,
                    _ => panic!("expected Earn command"),
                };
                assert_eq!(parsed, selector);
            }
        }
    }

    #[test]
    fn parses_multisig_transfers_and_preserves_single_signer_defaults() {
        let address = "0x1111111111111111111111111111111111111111";
        for operation in ["supply", "withdraw"] {
            for multisig in [false, true] {
                let mut args = vec!["hypecli", "earn", operation];
                if multisig {
                    args.extend(["--multi-sig-addr", address, "--local", "--amount", "100"]);
                }
                let cli = Cli::try_parse_from(args).unwrap();
                let Some(Command::Earn(EarnCmd::Supply(cmd) | EarnCmd::Withdraw(cmd))) =
                    cli.command
                else {
                    panic!("expected Earn transfer");
                };
                assert_eq!(cmd.token, "USDC");
                assert_eq!(cmd.local, multisig);
                assert_eq!(
                    cmd.multi_sig_addr,
                    multisig.then(|| address.parse().unwrap())
                );
                assert_eq!(cmd.amount, multisig.then_some(dec!(100)));
            }
            assert!(Cli::try_parse_from(["hypecli", "earn", operation, "--local"]).is_err());
        }
    }

    #[tokio::test]
    async fn multisig_earn_signatures_bind_account_nonce_and_action() {
        let signer = PrivateKeySigner::random();
        let nonce = 1_700_000_000_000;
        for chain in [Chain::Mainnet, Chain::Testnet] {
            for operation in [BorrowLendOperation::Supply, BorrowLendOperation::Withdraw] {
                for amount in [Some(dec!(100)), None] {
                    let payload = MultiSigPayload {
                        multi_sig_user: "0x1111111111111111111111111111111111111111".into(),
                        outer_signer: signer.address().to_string().to_lowercase(),
                        action: Box::new(Action::BorrowLend(BorrowLendAction {
                            operation,
                            token: 0,
                            amount,
                        })),
                    };
                    let signature = payload.sign(&signer, nonce, chain).await.unwrap();
                    assert_eq!(
                        payload.recover(&signature, nonce, chain).unwrap(),
                        signer.address()
                    );
                    assert_ne!(
                        payload.recover(&signature, nonce + 1, chain).unwrap(),
                        signer.address()
                    );

                    let mut changed = payload.clone();
                    changed.multi_sig_user = "0x2222222222222222222222222222222222222222".into();
                    assert_ne!(
                        changed.recover(&signature, nonce, chain).unwrap(),
                        signer.address()
                    );
                    let mut changed = payload.clone();
                    if let Action::BorrowLend(action) = changed.action.as_mut() {
                        action.amount = Some(dec!(101));
                    }
                    assert_ne!(
                        changed.recover(&signature, nonce, chain).unwrap(),
                        signer.address()
                    );
                }
            }
        }
    }

    #[test]
    fn supply_serializes_as_borrow_lend() {
        let action = Action::BorrowLend(BorrowLendAction {
            operation: BorrowLendOperation::Supply,
            token: 0,
            amount: Some(dec!(25)),
        });
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            serde_json::json!({
                "type": "borrowLend", "operation": "supply", "token": 0, "amount": "25"
            })
        );
    }

    #[test]
    fn withdraw_without_amount_sends_null() {
        let action = Action::BorrowLend(BorrowLendAction {
            operation: BorrowLendOperation::Withdraw,
            token: 0,
            amount: None,
        });
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            serde_json::json!({
                "type": "borrowLend", "operation": "withdraw", "token": 0, "amount": null
            })
        );
    }
}
