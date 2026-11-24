//! Shared signing options and submission for single-action commands.

use alloy::primitives::Address;
use clap::Args;
use hypersdk::hypercore::{
    HttpClient, NonceHandler,
    api::{Action, OkResponse, Response},
};

use crate::{
    SignerArgs,
    multisig::execute_multisig_action,
    utils::{find_signer, find_signers},
};

#[derive(Args, derive_more::Deref)]
pub struct ActionArgs {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Execute on behalf of this multi-sig wallet
    #[arg(long)]
    pub multi_sig_addr: Option<Address>,

    /// Require enough local signers without starting P2P gossip
    #[arg(long, requires = "multi_sig_addr")]
    pub local: bool,
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::*;
    use crate::{
        Cli, Command, dex::DexCmd, earn::EarnCmd, orders::OrderCmd, outcome::OutcomeCmd,
        prio::PrioCmd, validator::ValidatorCmd, vault::VaultCmd,
    };

    fn action_args(command: Command) -> ActionArgs {
        match command {
            Command::Send(cmd) => cmd.signer,
            Command::Earn(EarnCmd::Supply(cmd) | EarnCmd::Withdraw(cmd)) => cmd.signer,
            Command::Vault(VaultCmd::Deposit(cmd) | VaultCmd::Withdraw(cmd)) => cmd.signer,
            Command::Order(OrderCmd::Limit(cmd)) => cmd.signer,
            Command::Order(OrderCmd::Market(cmd)) => cmd.signer,
            Command::Order(OrderCmd::Cancel(cmd)) => cmd.signer,
            Command::Outcome(OutcomeCmd::Split(cmd)) => cmd.signer,
            Command::Outcome(OutcomeCmd::Merge(cmd)) => cmd.signer,
            Command::Outcome(OutcomeCmd::MergeQuestion(cmd)) => cmd.signer,
            Command::Outcome(OutcomeCmd::Negate(cmd)) => cmd.signer,
            Command::Prio(PrioCmd::Bid(cmd)) => cmd.signer,
            Command::Dex(DexCmd::Halt(cmd) | DexCmd::Resume(cmd)) => cmd.signer,
            Command::Dex(DexCmd::SubDeployer(cmd)) => cmd.signer,
            Command::Dex(DexCmd::Allow(cmd) | DexCmd::Disallow(cmd) | DexCmd::CancelAll(cmd)) => {
                cmd.signer
            }
            Command::Dex(DexCmd::ReduceOnly(cmd)) => cmd.star.signer,
            Command::Validator(ValidatorCmd::RotateSigner(cmd)) => cmd.signer,
            Command::Validator(ValidatorCmd::ChangeProfile(cmd)) => cmd.signer,
            Command::Validator(ValidatorCmd::Register(cmd)) => cmd.signer,
            Command::Validator(ValidatorCmd::Unregister(cmd)) => cmd.signer,
            Command::Validator(ValidatorCmd::Jail(cmd) | ValidatorCmd::Unjail(cmd)) => cmd.signer,
            Command::Validator(ValidatorCmd::StakeDeposit(cmd)) => cmd.signer,
            Command::Validator(ValidatorCmd::StakeWithdraw(cmd)) => cmd.signer,
            Command::Validator(ValidatorCmd::Delegate(cmd) | ValidatorCmd::Undelegate(cmd)) => {
                cmd.signer
            }
            Command::Validator(ValidatorCmd::ApproveAgent(cmd)) => cmd.signer,
            Command::Validator(ValidatorCmd::VoteRate(cmd)) => cmd.signer,
            _ => panic!("expected an action command"),
        }
    }

    #[test]
    fn actions_share_multisig_options_and_preserve_single_signer_defaults() {
        Cli::command().debug_assert();
        let address = "0x1111111111111111111111111111111111111111";
        let cases = [
            "send --token USDT0 --amount 10",
            "earn supply --token USDC",
            "earn withdraw --token USDT0",
            "vault deposit --vault 0x2222222222222222222222222222222222222222 --amount 10",
            "vault withdraw --vault 0x2222222222222222222222222222222222222222 --amount 10",
            "order limit --asset BTC --side buy --price 100 --size 0.01",
            "order market --asset BTC --side sell --slippage-price 100 --size 0.01",
            "order cancel --asset BTC --oid 123",
            "order cancel --asset BTC --cloid 0x11111111111111111111111111111111",
            "outcome split --outcome 20 --amount 10",
            "outcome merge --outcome 20",
            "outcome merge-question --question 3",
            "outcome negate --question 3 --outcome 20 --amount 10",
            "prio bid --max 1 --ip 127.0.0.1",
            "dex halt --coin xyz:SP500",
            "dex resume --coin xyz:SP500",
            "dex sub-deployer --dex xyz --user 0x2222222222222222222222222222222222222222 --permission setOracle",
            "dex sub-deployer --dex xyz --user 0x2222222222222222222222222222222222222222 --permission hip3Star:modifyApproval --revoke",
            "dex allow --dex test --user 0x2222222222222222222222222222222222222222",
            "dex disallow --dex test --user 0x2222222222222222222222222222222222222222",
            "dex reduce-only --dex test --user 0x2222222222222222222222222222222222222222",
            "dex reduce-only --dex test --user 0x2222222222222222222222222222222222222222 --off",
            "dex cancel-all --dex test --user 0x2222222222222222222222222222222222222222",
            "validator rotate-signer --new-signer 0x2222222222222222222222222222222222222222",
            "validator change-profile --name infinitefield.xyz --commission-bps 100",
            "validator register --ip 1.2.3.4 --name val --description desc --commission-bps 100 --consensus-signer 0x2222222222222222222222222222222222222222 --initial-stake 10000",
            "validator unregister",
            "validator jail",
            "validator unjail",
            "validator stake-deposit --amount 100",
            "validator stake-withdraw --amount 100",
            "validator delegate --validator 0x2222222222222222222222222222222222222222 --amount 100",
            "validator undelegate --validator 0x2222222222222222222222222222222222222222 --amount 100",
            "validator approve-agent --agent 0x2222222222222222222222222222222222222222 --name publisher",
            "validator vote-rate --rate 0.04",
        ];
        for case in cases {
            let mut args = vec!["hypecli"];
            args.extend(case.split_whitespace());
            let parsed = action_args(Cli::try_parse_from(&args).unwrap().command.unwrap());
            assert!(parsed.multi_sig_addr.is_none(), "{case}");
            assert!(!parsed.local, "{case}");

            args.push("--local");
            assert!(Cli::try_parse_from(&args).is_err(), "{case}");
            args.extend(["--multi-sig-addr", address, "--chain", "testnet"]);
            let parsed = action_args(Cli::try_parse_from(&args).unwrap().command.unwrap());
            assert_eq!(
                parsed.multi_sig_addr,
                Some(address.parse().unwrap()),
                "{case}"
            );
            assert!(parsed.local, "{case}");
            assert_eq!(parsed.chain, hypersdk::hypercore::Chain::Testnet, "{case}");
        }
    }
}

impl ActionArgs {
    /// Build the action after discovering signers, so its nonce is fresh and
    /// internal transfers default to the account whose funds are being moved.
    pub async fn execute(
        &self,
        client: HttpClient,
        build: impl FnOnce(Address, u64) -> Action,
    ) -> anyhow::Result<OkResponse> {
        self.try_execute(client, |account, nonce| Ok(build(account, nonce)))
            .await
    }

    pub async fn try_execute(
        &self,
        client: HttpClient,
        build: impl FnOnce(Address, u64) -> anyhow::Result<Action>,
    ) -> anyhow::Result<OkResponse> {
        if let Some(account) = self.multi_sig_addr {
            let config = client.multi_sig_config(account).await?;
            let signers = find_signers(&self.signer, &config.authorized_users).await?;
            let nonce = NonceHandler::default().next();
            let action = build(account, nonce)?;
            return execute_multisig_action(
                account,
                client,
                signers,
                action,
                nonce,
                &config,
                self.local,
                &self.trezor,
            )
            .await;
        }

        let signer = find_signer(&self.signer, None).await?;
        let nonce = NonceHandler::default().next();
        let request = build(signer.address(), nonce)?
            .sign(&signer, nonce, None, None, self.chain)
            .await?;
        match client.send(request).await? {
            Response::Ok(response) => Ok(response),
            Response::Err(error) => anyhow::bail!("{error}"),
        }
    }

    pub async fn execute_default(
        &self,
        client: HttpClient,
        build: impl FnOnce(Address, u64) -> Action,
    ) -> anyhow::Result<()> {
        Response::Ok(self.execute(client, build).await?).into_default()
    }

    pub async fn try_execute_default(
        &self,
        client: HttpClient,
        build: impl FnOnce(Address, u64) -> anyhow::Result<Action>,
    ) -> anyhow::Result<()> {
        Response::Ok(self.try_execute(client, build).await?).into_default()
    }
}
