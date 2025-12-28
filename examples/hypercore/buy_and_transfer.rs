use std::{
    env,
    future::poll_fn,
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use futures::{FutureExt, StreamExt, stream::FuturesUnordered};
use hypersdk::hypercore::{
    self as hypercore, Cloid,
    types::{BatchOrder, OrderGrouping, OrderRequest, OrderTypePlacement, TimeInForce},
};
use rust_decimal::{Decimal, dec};
use tokio::{sync::oneshot, time::interval};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Token to transfer
    #[arg(short, long)]
    token: String,
    /// Limit price
    #[arg(short, long)]
    price: Decimal,
    /// Amount to send
    #[arg(short, long)]
    amount: Decimal,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv()?;

    let private_key = env::var("PRIVATE_KEY")?;
    let signer = hypercore::PrivateKeySigner::from_str(&private_key)?;

    let _ = simple_logger::init_with_level(log::Level::Debug);

    let args = Cli::parse();

    let client = hypercore::mainnet();

    let markets = client.spot().await?;
    let market = markets
        .iter()
        .find(|market| market.tokens[0].name == args.token && market.tokens[1].name == "USDC")
        .ok_or(anyhow::anyhow!("{} not found", args.token))?
        .clone();

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    log::info!(
        "Sending order to {} {} @ {} at nonce {nonce}",
        market.index,
        args.amount,
        args.price
    );

    let future = client.place(
        &signer,
        BatchOrder {
            orders: vec![OrderRequest {
                asset: market.index,
                is_buy: true,
                limit_px: args.price,
                sz: args.amount,
                reduce_only: false,
                order_type: OrderTypePlacement::Limit {
                    tif: TimeInForce::Ioc,
                },
                cloid: Cloid::random(),
            }],
            grouping: OrderGrouping::Na,
        },
        nonce,
        None,
        None,
    );

    let (tx, mut rx) = oneshot::channel();
    let join = tokio::spawn(async move {
        let mut futures = FuturesUnordered::new();
        let mut ticker = interval(Duration::from_millis(50));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    futures.push(client.transfer_to_evm(
                        &signer,
                        market.tokens[0].clone(),
                        args.amount * dec!(0.9993),
                        nonce + 1,
                    ));
                }
                _ = poll_fn(|cx| {
                    rx.poll_unpin(cx)
                }) => {
                    break;
                }
            }
        }

        while let Some(res) = futures.next().await {
            log::debug!("res: {res:?}");
        }
    });

    tokio::spawn(async move {
        let res = future.await;
        let _ = tx.send(());
        if let Ok(placements) = res {
            let res = &placements[0];
            log::debug!("Result: {res:?}");
            if let hypercore::types::OrderResponseStatus::Filled {
                total_sz,
                avg_px: _,
                oid: _,
            } = res
            {
                log::info!("Successful taker order, sending {total_sz} to EVM");
            }
        }
    });

    let _ = join.await;

    Ok(())
}
