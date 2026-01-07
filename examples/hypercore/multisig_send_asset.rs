use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use hypersdk::{
    Address,
    hypercore::{
        self as hypercore, Chain, PrivateKeySigner,
        types::{SendAsset, SendToken},
    },
};
use rust_decimal::dec;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Private keys of the signers (must be authorized on the multisig wallet)
    #[arg(long)]
    private_key: Vec<String>,

    /// Multisig wallet address (the source of funds)
    #[arg(long)]
    multisig_address: Address,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    // Initialize logger for debugging
    let _ = simple_logger::init_with_level(log::Level::Debug);

    // Create HTTP client for the specified chain
    let client = hypercore::HttpClient::new(Chain::Testnet);

    // Parse all private keys into signers
    let signers: Vec<_> = args
        .private_key
        .iter()
        .map(|key| PrivateKeySigner::from_str(key.as_str()).unwrap())
        .collect();

    println!("Multisig wallet: {}", args.multisig_address);

    // Fetch spot token metadata to get the token information
    let tokens = client.spot_tokens().await?;
    let token = tokens.iter().find(|t| t.name == "USDC").unwrap();

    println!(
        "Found token: {} (index: {}, token_id: {})",
        token.name, token.index, token.token_id
    );

    // Generate unique nonce (timestamp in milliseconds)
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Create the asset transfer action
    let send_asset = SendAsset {
        // Recipient address
        destination: "0x071749F7D58dFD985bDEA9cC14C429c530936432"
            .parse()
            .unwrap(),
        // Source DEX/balance ("" = perp, "spot" = spot)
        source_dex: "spot".to_owned(),
        // Destination DEX/balance ("" = perp, "spot" = spot)
        destination_dex: "spot".to_owned(),
        // Token to transfer
        token: SendToken(token.clone()),
        // Subaccount to send from (empty for main account)
        from_sub_account: "".to_string(),
        // Amount to transfer
        amount: dec!(1.0),
        // Unique transaction nonce
        nonce,
    };

    client
        .multi_sig(&signers[0], args.multisig_address, nonce)
        .signers(&signers)
        .send_asset(send_asset)
        .await?;

    Ok(())
}
