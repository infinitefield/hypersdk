//! Query APY for a MetaMorpho vault.
//!
//! This example fetches the current APY for a MetaMorpho vault on HyperEVM. MetaMorpho vaults
//! are automated vault strategies that optimize yield across multiple Morpho markets. This is
//! useful for comparing vault performance and building yield aggregators.
//!
//! # Usage
//!
//! ```bash
//! # Query vault APY
//! cargo run --example vault_apy -- \
//!   --contract-address 0x207ccaE51Ad2E1C240C4Ab4c94b670D438d2201C \
//!   --rpc-url https://rpc.hyperliquid.xyz/evm
//!
//! # Custom vault address
//! cargo run --example vault_apy -- \
//!   --contract-address 0x... \
//!   --rpc-url https://rpc.hyperliquid.xyz/evm
//! ```
//!
//! # What it does
//!
//! 1. Connects to HyperEVM via RPC
//! 2. Queries the MetaMorpho vault contract
//! 3. Calculates current vault APY based on:
//!    - Underlying market allocations
//!    - Current utilization rates
//!    - Fee structures
//! 4. Displays the effective vault APY
//!
//! # Output
//!
//! ```text
//! Connecting to RPC endpoint: https://rpc.hyperliquid.xyz/evm
//! apy: 6.45
//! ```
//!
//! # MetaMorpho Vaults
//!
//! MetaMorpho vaults automatically:
//! - Allocate capital across multiple Morpho markets
//! - Rebalance to optimize yield
//! - Manage risk exposure
//! - Charge performance fees
//!
//! The APY shown is the net rate depositors earn after fees.

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
    #[arg(short, long, default_value = "https://rpc.hyperliquid.xyz/evm")]
    rpc_url: String,
}

// https://github.com/morpho-org/metamorpho-v1.1/blob/main/src/MetaMorphoV1_1.sol#L796

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    println!("Connecting to RPC endpoint: {}", args.rpc_url);

    let provider = DynProvider::new(hyperevm::mainnet_with_url(&args.rpc_url).await?);
    let client = MetaClient::new(provider);

    // Use the simplified apy method that returns SimpleVaultApy
    let vault = client.apy_simple(args.contract_address).await?;

    println!("APY: {:.2}%", vault.apy() * 100.0);
    println!("Gross APY: {:.2}%", vault.gross_apy() * 100.0);
    println!("Fee: {:.2}%", vault.fee * 100.0);
    println!("Total Deposits: {:.2}", vault.total_deposits);
    println!("Markets: {}", vault.market_count());

    Ok(())
}
