mod multisig;
mod utils;

use std::io::Write;
use std::io::stdout;

use clap::Args;
use clap::{Parser, Subcommand};
use enum_dispatch::enum_dispatch;
use hypersdk::Address;
use hypersdk::Decimal;
use hypersdk::hypercore;
use hypersdk::hypercore::Chain;
use hypersdk::hyperevm;
use hypersdk::hyperevm::morpho;
use iroh_tickets::endpoint::EndpointTicket;

#[derive(Parser)]
#[command(author, version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    // with_url: Url,
}

#[enum_dispatch]
trait Run {
    async fn run(self) -> anyhow::Result<()>;
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
#[enum_dispatch(Run)]
enum Commands {
    /// List perpetual markets
    Perps(PerpsCmd),
    /// List spot markets
    Spot(SpotCmd),
    /// Gather spot balances for a user.
    SpotBalances(SpotBalancesCmd),
    /// Query an addresses' morpho balance
    MorphoPosition(MorphoPositionCmd),
    /// Sign
    #[command(subcommand)]
    Multisig(MultiSigCmd),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    args.command.run().await
}

/// Multi-sig regardless of your location.
///
/// This commands setups up a peer-to-peer communication
/// to allow for decentralized multi-sig.
#[derive(Subcommand)]
#[enum_dispatch(Run)]
enum MultiSigCmd {
    Sign(MultiSigSign),
    SendAsset(MultiSigSendAsset),
}

#[derive(Args)]
struct MultiSigCommon {
    #[arg(long)]
    pub private_key: Option<String>,
    /// Foundry keystore.
    #[arg(long)]
    pub keystore: Option<String>,
    /// Keystore password. Otherwise it'll be prompted.
    #[arg(long)]
    pub password: Option<String>,
    /// Multi-sig wallet.
    #[arg(long)]
    pub multi_sig_addr: Address,
    #[arg(long)]
    pub chain: Chain,
}

#[derive(Args, derive_more::Deref)]
struct MultiSigSendAsset {
    #[deref]
    #[command(flatten)]
    pub common: MultiSigCommon,
    /// Destination address.
    #[arg(long)]
    pub to: Address,
    /// Token to send.
    #[arg(long)]
    pub token: String,
    /// Amount to send.
    #[arg(long)]
    pub amount: Decimal,
    /// Source DEX. Can be "spot" or a dex
    #[arg(long)]
    pub source: Option<String>,
    /// Destination DEX. Can be "spot" or a dex
    #[arg(long)]
    pub dest: Option<String>,
}

impl Run for MultiSigSendAsset {
    async fn run(self) -> anyhow::Result<()> {
        multisig::send_asset(self).await
    }
}

#[derive(Args, derive_more::Deref)]
struct MultiSigSign {
    #[deref]
    #[command(flatten)]
    pub common: MultiSigCommon,
    /// Endpoint to connect to.
    #[arg(long)]
    pub connect: EndpointTicket,
}

impl Run for MultiSigSign {
    async fn run(self) -> anyhow::Result<()> {
        multisig::sign(self).await
    }
}

#[derive(Args)]
struct PerpsCmd;

impl Run for PerpsCmd {
    async fn run(self) -> anyhow::Result<()> {
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

#[derive(Args)]
struct SpotCmd;

impl Run for SpotCmd {
    async fn run(self) -> anyhow::Result<()> {
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

#[derive(Args)]
struct SpotBalancesCmd {
    #[arg(short, long)]
    user: Address,
}

impl Run for SpotBalancesCmd {
    async fn run(self) -> anyhow::Result<()> {
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

#[derive(Args)]
struct MorphoPositionCmd {
    /// Morpho's contract address.
    #[arg(
        short,
        long,
        default_value = "0x68e37dE8d93d3496ae143F2E900490f6280C57cD"
    )]
    contract: Address,
    /// RPC endpoint
    #[arg(short, long, default_value = "https://rpc.hyperliquid.xyz/evm")]
    rpc_url: String,
    /// Morpho market
    #[arg(short, long)]
    market: morpho::MarketId,
    /// Target user
    #[arg(short, long)]
    user: Address,
}

impl Run for MorphoPositionCmd {
    async fn run(self) -> anyhow::Result<()> {
        let provider = hyperevm::mainnet_with_url(&self.rpc_url).await?;
        let client = hyperevm::morpho::Client::new(provider);
        let morpho = client.instance(self.contract);
        let position = morpho.position(self.market, self.user).call().await?;

        let mut writer = tabwriter::TabWriter::new(stdout());

        writeln!(&mut writer, "borrow shares\tcollateral\tsupply shares")?;
        writeln!(
            &mut writer,
            "{}\t{}\t{}",
            position.borrowShares, position.collateral, position.supplyShares
        )?;

        writer.flush()?;

        Ok(())
    }
}
