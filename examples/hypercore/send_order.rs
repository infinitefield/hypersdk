use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use hypersdk::hypercore::{
    self as hypercore, Cloid,
    types::{BatchOrder, OrderGrouping, OrderRequest, OrderTypePlacement, TimeInForce},
};
use rust_decimal::dec;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    private_key: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = simple_logger::init_with_level(log::Level::Debug);

    let args = Cli::parse();

    let client = hypercore::mainnet();
    let signer = hypercore::PrivateKeySigner::from_str(&args.private_key)?;

    let perps = client.perps().await?;
    let btc = perps.iter().find(|perp| perp.name == "BTC").expect("btc");

    let resp = client
        .place(
            &signer,
            BatchOrder {
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
            },
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            None,
            None,
        )
        .await?;

    println!("{resp:?}");

    Ok(())
}
