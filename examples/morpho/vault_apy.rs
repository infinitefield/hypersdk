use clap::Parser;
use hypersdk::{
    Address,
    hyperevm::{self, DynProvider, morpho::MetaClient},
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Address of the vault contract.
    #[arg(
        short,
        long,
        default_value = "0x207ccaE51Ad2E1C240C4Ab4c94b670D438d2201C"
    )]
    contract_address: Address,
    /// RPC url
    #[arg(short, long, default_value = "http://127.0.0.1:8545")]
    rpc_url: String,
}

// https://github.com/morpho-org/metamorpho-v1.1/blob/main/src/MetaMorphoV1_1.sol#L796

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    println!("Connecting to RPC endpoint: {}", args.rpc_url);

    let provider = DynProvider::new(hyperevm::mainnet_with_url(&args.rpc_url).await?);
    let vault = MetaClient::new(provider).apy(args.contract_address).await?;

    println!("apy: {}", vault.apy() * 100.0);

    Ok(())
}
