mod balances;
mod markets;
mod morpho;
mod multisig;
mod utils;

use clap::Parser;

use balances::SpotBalancesCmd;
use markets::{PerpsCmd, SpotCmd};
use morpho::MorphoPositionCmd;
use multisig::MultiSigCmd;

/// Main CLI structure for hypecli.
///
/// Parses command-line arguments and dispatches to the appropriate subcommand.
#[derive(Parser)]
#[command(author, version)]
#[allow(clippy::large_enum_variant)]
enum Cli {
    /// List perpetual markets
    Perps(PerpsCmd),
    /// List spot markets
    Spot(SpotCmd),
    /// Gather spot balances for a user.
    SpotBalances(SpotBalancesCmd),
    /// Query an addresses' morpho balance
    MorphoPosition(MorphoPositionCmd),
    /// Multi-sig commands
    #[command(subcommand)]
    Multisig(MultiSigCmd),
}

impl Cli {
    async fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Perps(cmd) => cmd.run().await,
            Self::Spot(cmd) => cmd.run().await,
            Self::SpotBalances(cmd) => cmd.run().await,
            Self::MorphoPosition(cmd) => cmd.run().await,
            Self::Multisig(cmd) => cmd.run().await,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    args.run().await
}
