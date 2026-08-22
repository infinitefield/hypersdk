//! HIP-4 outcome token commands.
//!
//! This module provides commands for splitting, merging, and negating
//! outcome tokens, plus a listing command to discover outcome and
//! question IDs from the `outcomeMeta` info endpoint.

use std::io::{Write, stdout};

use clap::{Args, Subcommand};
use hypersdk::{
    Decimal,
    hypercore::{Chain, HttpClient, NonceHandler},
};

use crate::SignerArgs;
use crate::utils::find_signer_sync;

/// HIP-4 outcome token commands.
#[derive(Subcommand)]
pub enum OutcomeCmd {
    /// List outcome markets and questions with their IDs
    List(OutcomeListCmd),
    /// Split the quote token into one share of each side of an outcome
    Split(OutcomeSplitCmd),
    /// Merge matching shares of an outcome back into the quote token
    Merge(OutcomeMergeCmd),
    /// Merge a full set of outcomes within a question back into the quote token
    MergeQuestion(OutcomeMergeQuestionCmd),
    /// Convert shares of an outcome into the complementary basket within a question
    Negate(OutcomeNegateCmd),
}

impl OutcomeCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            OutcomeCmd::List(cmd) => cmd.run().await,
            OutcomeCmd::Split(cmd) => cmd.run().await,
            OutcomeCmd::Merge(cmd) => cmd.run().await,
            OutcomeCmd::MergeQuestion(cmd) => cmd.run().await,
            OutcomeCmd::Negate(cmd) => cmd.run().await,
        }
    }
}

/// Arguments for the outcome listing query.
#[derive(Args)]
pub struct OutcomeListCmd {
    /// Chain to query.
    #[arg(long, default_value = "mainnet")]
    pub chain: Chain,
}

impl OutcomeListCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let client = HttpClient::new(self.chain);
        let meta = client.outcome_meta().await?;

        let mut writer = tabwriter::TabWriter::new(stdout());

        let _ = writeln!(&mut writer, "outcome\tname\tsides\tdescription");
        for outcome in &meta.outcomes {
            let sides = outcome
                .side_specs
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join("/");
            let _ = writeln!(
                &mut writer,
                "{}\t{}\t{}\t{}",
                outcome.outcome, outcome.name, sides, outcome.description,
            );
        }

        let _ = writeln!(&mut writer);
        let _ = writeln!(
            &mut writer,
            "question\tname\toutcomes\tsettled\tfallback\tdescription"
        );
        for question in &meta.questions {
            let ids = |ids: &[u32]| {
                ids.iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let fallback = question
                .fallback_outcome
                .map(|o| o.to_string())
                .unwrap_or_else(|| "-".to_string());
            let _ = writeln!(
                &mut writer,
                "{}\t{}\t{}\t{}\t{}\t{}",
                question.question,
                question.name,
                ids(&question.named_outcomes),
                ids(&question.settled_named_outcomes),
                fallback,
                question.description,
            );
        }

        let _ = writer.flush();

        Ok(())
    }
}

/// Arguments for splitting the quote token into outcome shares.
#[derive(Args, derive_more::Deref)]
pub struct OutcomeSplitCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Outcome ID (see `hypecli outcome list`)
    #[arg(long)]
    pub outcome: u32,

    /// Amount of the quote token to split
    #[arg(long)]
    pub amount: Decimal,
}

impl OutcomeSplitCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let signer = find_signer_sync(&self.signer)?;
        let client = HttpClient::new(self.signer.chain);
        let nonce = NonceHandler::default().next();
        println!(
            "Splitting {} of the quote token into outcome {}",
            self.amount, self.outcome
        );
        client
            .split_outcome(&signer, self.outcome, self.amount, nonce, None, None)
            .await?;
        println!("Split successfully.");
        Ok(())
    }
}

/// Arguments for merging outcome shares back into the quote token.
#[derive(Args, derive_more::Deref)]
pub struct OutcomeMergeCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Outcome ID (see `hypecli outcome list`)
    #[arg(long)]
    pub outcome: u32,

    /// Amount of matching shares to merge. Omit to merge the maximum available.
    #[arg(long)]
    pub amount: Option<Decimal>,
}

impl OutcomeMergeCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let signer = find_signer_sync(&self.signer)?;
        let client = HttpClient::new(self.signer.chain);
        let nonce = NonceHandler::default().next();
        match self.amount {
            Some(amount) => println!("Merging {} shares of outcome {}", amount, self.outcome),
            None => println!("Merging all available shares of outcome {}", self.outcome),
        }
        client
            .merge_outcome(&signer, self.outcome, self.amount, nonce, None, None)
            .await?;
        println!("Merged successfully.");
        Ok(())
    }
}

/// Arguments for merging a question's full outcome set back into the quote token.
#[derive(Args, derive_more::Deref)]
pub struct OutcomeMergeQuestionCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Question ID (see `hypecli outcome list`)
    #[arg(long)]
    pub question: u32,

    /// Amount to merge. Omit to merge the maximum available.
    #[arg(long)]
    pub amount: Option<Decimal>,
}

impl OutcomeMergeQuestionCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let signer = find_signer_sync(&self.signer)?;
        let client = HttpClient::new(self.signer.chain);
        let nonce = NonceHandler::default().next();
        match self.amount {
            Some(amount) => println!("Merging {} across question {}", amount, self.question),
            None => println!(
                "Merging all available outcome sets of question {}",
                self.question
            ),
        }
        client
            .merge_outcome_question(&signer, self.question, self.amount, nonce, None, None)
            .await?;
        println!("Merged successfully.");
        Ok(())
    }
}

/// Arguments for negating an outcome within a question.
#[derive(Args, derive_more::Deref)]
pub struct OutcomeNegateCmd {
    #[deref]
    #[command(flatten)]
    pub signer: SignerArgs,

    /// Question ID (see `hypecli outcome list`)
    #[arg(long)]
    pub question: u32,

    /// Outcome ID to negate within the question
    #[arg(long)]
    pub outcome: u32,

    /// Amount of shares to negate
    #[arg(long)]
    pub amount: Decimal,
}

impl OutcomeNegateCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let signer = find_signer_sync(&self.signer)?;
        let client = HttpClient::new(self.signer.chain);
        let nonce = NonceHandler::default().next();
        println!(
            "Negating {} shares of outcome {} within question {}",
            self.amount, self.outcome, self.question
        );
        client
            .negate_outcome(
                &signer,
                self.question,
                self.outcome,
                self.amount,
                nonce,
                None,
                None,
            )
            .await?;
        println!("Negated successfully.");
        Ok(())
    }
}
