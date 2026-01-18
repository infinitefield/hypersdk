//! Shows how placing, modifying and cancelling works.
//!
//! This example places an order, modifies it and then cancels it by oid.

use clap::Parser;
use hypersdk::hypercore::{
    self as hypercore, BatchCancel, BatchModify, Cancel, Cloid, Modify, NonceHandler, OidOrCloid,
    types::{BatchOrder, OrderGrouping, OrderRequest, OrderTypePlacement, TimeInForce},
};
use rust_decimal::dec;

use crate::credentials::Credentials;

mod credentials;

#[derive(Parser, Debug, derive_more::Deref)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[deref]
    #[command(flatten)]
    common: Credentials,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = simple_logger::init_with_level(log::Level::Debug);

    let args = Cli::parse();
    let signer = args.get()?;

    let client = hypercore::mainnet();
    let role = client.user_role(signer.address()).await?;

    let perps = client.perps().await?;
    let btc = perps.iter().find(|perp| perp.name == "BTC").expect("btc");

    let vault_address = match role {
        hypercore::UserRole::SubAccount { master } => Some(master),
        _ => None,
    };

    let nonce = NonceHandler::default();

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
            nonce.next(),
            vault_address,
            None,
        )
        .await?;

    match &resp[0] {
        hypercore::OrderResponseStatus::Resting { oid, cloid: _cloid } => {
            let resp = client
                .modify(
                    &signer,
                    BatchModify {
                        modifies: vec![Modify {
                            oid: OidOrCloid::Left(*oid),
                            order: OrderRequest {
                                asset: btc.index,
                                is_buy: true,
                                limit_px: dec!(88_000),
                                sz: dec!(0.01),
                                reduce_only: false,
                                order_type: OrderTypePlacement::Limit {
                                    tif: TimeInForce::Alo,
                                },
                                cloid: Cloid::random(),
                            },
                        }],
                    },
                    nonce.next(),
                    vault_address,
                    None,
                )
                .await?;

            match &resp[0] {
                hypercore::OrderResponseStatus::Resting { oid, cloid: _cloid } => {
                    client
                        .cancel(
                            &signer,
                            BatchCancel {
                                cancels: vec![Cancel {
                                    asset: btc.index,
                                    oid: *oid,
                                }],
                            },
                            nonce.next(),
                            vault_address,
                            None,
                        )
                        .await?;
                }
                _ => {
                    println!("failed amending order: {resp:?}")
                }
            }
        }
        _ => {
            println!("failed placing order: {resp:?}")
        }
    }

    Ok(())
}
