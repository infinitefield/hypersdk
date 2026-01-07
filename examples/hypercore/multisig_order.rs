use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use hypersdk::{
    Address,
    hypercore::{
        self as hypercore, Chain, Cloid, PrivateKeySigner,
        types::{BatchOrder, OrderGrouping, OrderRequest, OrderTypePlacement, TimeInForce},
    },
};
use rust_decimal::dec;

/// Example demonstrating how to execute a multisig order on Hyperliquid.
///
/// This example shows how to use Hyperliquid's L1 multisig functionality to place an order
/// that requires multiple signers to authorize the transaction. Multisig orders are useful
/// for implementing custody solutions, DAOs, or any scenario requiring multiple parties to
/// approve trading actions.
///
/// # Multisig Flow
///
/// 1. Create the trading action (e.g., placing an order)
/// 2. Collect signatures from all required signers
/// 3. Submit the multisig transaction with all signatures
/// 4. The exchange verifies all signatures match the multisig wallet configuration
///
/// # Usage
///
/// ```bash
/// cargo run --example multisig_order -- \
///   --private-key KEY1 \
///   --private-key KEY2 \
///   --multisig-address 0x... \
///   --chain mainnet
/// ```
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Private keys to sign
    #[arg(long)]
    private_key: Vec<String>,
    /// Multisig wallet address
    #[arg(long)]
    multisig_address: Address,
    #[arg(long)]
    chain: Chain,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    let _ = simple_logger::init_with_level(log::Level::Debug);

    let client = hypercore::HttpClient::new(args.chain);

    // Parse all private keys into signers
    // Each signer must be authorized on the multisig wallet
    let signers: Vec<_> = args
        .private_key
        .iter()
        .map(|key| PrivateKeySigner::from_str(key.as_str()).unwrap())
        .collect();

    // Fetch BTC perpetual market information
    // We need the market index to place orders
    let perps = client.perps().await?;
    let btc = perps.iter().find(|perp| perp.name == "BTC").expect("btc");

    // Create the order action to be executed via multisig
    // This order will buy 0.01 BTC at $87,000 with ALO (Add Liquidity Only) time-in-force
    let order = BatchOrder {
        orders: vec![OrderRequest {
            asset: btc.index,
            is_buy: true,
            limit_px: dec!(87_000),
            sz: dec!(0.01),
            reduce_only: false,
            order_type: OrderTypePlacement::Limit {
                tif: TimeInForce::Alo,
            },
            cloid: Cloid::random(),
        }],
        grouping: OrderGrouping::Na,
    };

    // Generate a unique nonce for this transaction
    // Using current timestamp ensures uniqueness and prevents replay attacks
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Execute multisig order
    let resp = client
        .multi_sig(&signers[0], args.multisig_address, nonce)
        .signers(&signers)
        .place(order, None, None)
        .await?;

    println!("Multisig order response: {resp:?}");

    Ok(())
}
