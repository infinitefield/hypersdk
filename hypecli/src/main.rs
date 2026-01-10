mod balances;
mod markets;
mod morpho;
mod multisig;
mod to_multisig;
mod utils;

use balances::SpotBalancesCmd;
use clap::{Args, Parser};
use hypersdk::hypercore::Chain;
use markets::{PerpsCmd, SpotCmd};
use morpho::MorphoPositionCmd;
use multisig::MultiSigCmd;
use to_multisig::ToMultiSigCmd;

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
    /// Convert a regular user to a multi-sig user
    ToMultisig(ToMultiSigCmd),
}

impl Cli {
    async fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Perps(cmd) => cmd.run().await,
            Self::Spot(cmd) => cmd.run().await,
            Self::SpotBalances(cmd) => cmd.run().await,
            Self::MorphoPosition(cmd) => cmd.run().await,
            Self::Multisig(cmd) => cmd.run().await,
            Self::ToMultisig(cmd) => cmd.run().await,
        }
    }
}

/// Common arguments for multi-signature commands.
///
/// These arguments are shared across all multi-sig operations to specify
/// the signer credentials and target multi-sig wallet.
#[derive(Args)]
pub struct SignerArgs {
    /// Private key for signing (hex format).
    #[arg(long)]
    pub private_key: Option<String>,
    /// Foundry keystore.
    #[arg(long)]
    pub keystore: Option<String>,
    /// Keystore password. Otherwise it'll be prompted.
    #[arg(long)]
    pub password: Option<String>,
    /// Target chain for the operation.
    #[arg(long)]
    pub chain: Chain,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    args.run().await
}
