use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use hypersdk::{
    Address,
    hypercore::{
        self as hypercore, Chain, Cloid, PrivateKeySigner,
        types::{Action, BatchOrder, OrderGrouping, OrderRequest, OrderTypePlacement, TimeInForce},
    },
};
use rust_decimal::dec;

/// Example demonstrating how to execute a multisig order.
///
/// This example shows how to use Hyperliquid's L1 multisig functionality to place an order
/// that requires multiple signers to authorize the transaction.
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

    let signers: Vec<_> = args
        .private_key
        .iter()
        .map(|key| PrivateKeySigner::from_str(key.as_str()).unwrap())
        .collect();

    // Get BTC perpetual market
    let perps = client.perps().await?;
    let btc = perps.iter().find(|perp| perp.name == "BTC").expect("btc");

    // Create the order action
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

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Execute multisig order
    // The signature chain ID is automatically determined by the client's chain (mainnet/testnet)
    let resp = client
        .multi_sig(
            &signers[0],
            args.multisig_address,
            &signers,
            Action::Order(order),
            nonce,
        )
        .await?;

    println!("Multisig order response: {resp:?}");

    Ok(())
}
