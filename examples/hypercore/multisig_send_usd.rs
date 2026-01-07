use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use hypersdk::{
    Address, Decimal,
    hypercore::{self as hypercore, Chain, PrivateKeySigner, types::UsdSend},
};

/// Example demonstrating how to execute a multisig USDC transfer on Hyperliquid.
///
/// This example shows how to use Hyperliquid's L1 multisig functionality to send USDC
/// from a multisig account. This requires multiple signers to authorize the transfer,
/// making it suitable for custody solutions, treasury management, DAOs, or any scenario
/// requiring multiple parties to approve fund movements.
///
/// # Multisig Flow
///
/// 1. Create the USDC transfer action (UsdSend)
/// 2. Each signer signs the EIP-712 typed data
/// 3. Collect all signatures into a MultiSigAction
/// 4. Submit the multisig transaction with all signatures
/// 5. The exchange verifies all signatures match the multisig wallet configuration
///
/// # Signing Method
///
/// Unlike orders (which use RMP/MessagePack hashing), USDC transfers use EIP-712 typed data.
/// This provides a more human-readable representation in wallet UIs, showing:
/// - Destination address
/// - Transfer amount
/// - Timestamp
/// - Chain (mainnet/testnet)
///
/// # Usage
///
/// ```bash
/// # Send 100 USDC from multisig to recipient
/// cargo run --example multisig_send_usd -- \
///   --private-key KEY1 \
///   --private-key KEY2 \
///   --private-key KEY3 \
///   --multisig-address 0x... \
///   --destination 0x... \
///   --amount 100 \
///   --chain mainnet
/// ```
///
/// # Security Notes
///
/// - All private keys must correspond to authorized signers on the multisig wallet
/// - The multisig wallet must have been configured on Hyperliquid beforehand
/// - The wallet must have sufficient USDC balance for the transfer
/// - Each transaction requires a unique nonce (timestamp is used for this)
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Private keys of the signers (must be authorized on the multisig wallet)
    #[arg(long)]
    private_key: Vec<String>,

    /// Multisig wallet address (the source of funds)
    #[arg(long)]
    multisig_address: Address,

    /// Destination address (recipient of USDC)
    #[arg(long)]
    destination: Address,

    /// Amount of USDC to send
    #[arg(long)]
    amount: Decimal,

    /// Chain to execute on (mainnet or testnet)
    #[arg(long)]
    chain: Chain,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    // Initialize logger for debugging
    let _ = simple_logger::init_with_level(log::Level::Debug);

    // Create HTTP client for the specified chain
    let client = hypercore::HttpClient::new(args.chain);

    // Parse all private keys into signers
    // Each signer must be authorized on the multisig wallet
    let signers: Vec<_> = args
        .private_key
        .iter()
        .map(|key| PrivateKeySigner::from_str(key.as_str()).unwrap())
        .collect();

    println!("Multisig wallet: {}", args.multisig_address);
    println!("Destination: {}", args.destination);
    println!("Amount: {} USDC", args.amount);
    println!("Number of signers: {}", signers.len());

    // Generate timestamps for nonce and transfer time
    // Both use the current timestamp in milliseconds
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Create the USDC transfer action
    // This specifies where to send, how much, and when
    let usd_send = UsdSend {
        // Recipient address
        destination: args.destination,
        // Amount to transfer (USDC has 6 decimals on-chain, but we use regular decimal)
        amount: args.amount,
        // Timestamp of the transfer (prevents replay attacks)
        time: now,
    };

    println!("\nInitiating multisig USDC transfer...");
    println!("Transfer time: {}", now);

    // Execute multisig USDC transfer
    // 1. First signer is the lead (submits the transaction)
    // 2. All signers (including lead) sign the typed data
    // 3. Signatures are collected and verified
    // 4. Transaction is submitted to the exchange
    client
        .multi_sig(&signers[0], args.multisig_address, now)
        .signers(&signers)
        .send_usdc(usd_send)
        .await?;

    println!("\n✅ Multisig USDC transfer successful!");
    println!(
        "Sent {} USDC from {} to {}",
        args.amount, args.multisig_address, args.destination
    );

    Ok(())
}
