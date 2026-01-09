use std::{
    io::{Read, Write, stdin, stdout},
    time::Duration,
};

use futures::StreamExt;
use hypersdk::hypercore::{
    self, HttpClient, NonceHandler, SendAsset, SendToken, Signature,
    raw::{Action, MultiSigAction, MultiSigPayload},
};
use indicatif::{ProgressBar, ProgressStyle};
use iroh_gossip::api::Event;
use serde::{Deserialize, Serialize};
use tokio::signal::ctrl_c;

use crate::{
    MultiSigSendAsset, MultiSigSign,
    utils::{self, find_signer, make_topic},
};

#[derive(Serialize, Deserialize)]
enum Message {
    Action(u64, MultiSigPayload),
    Signature(Signature),
}

const CONNECTING_STRINGS: &[&str] = &[
    "Connecting",
    "COnnecting",
    "CoNnecting",
    "ConNecting",
    "ConnEcting",
    "ConneCting",
    "ConnecTing",
    "ConnectIng",
    "ConnectiNg",
    "ConnectinG",
];

/// Initiate sending an asset.
pub async fn send_asset(cmd: MultiSigSendAsset) -> anyhow::Result<()> {
    let hl = HttpClient::new(cmd.chain);
    let multisig_config = hl.multi_sig_config(cmd.multi_sig_addr).await?;
    let signer = find_signer(&cmd.common, &multisig_config.authorized_users).await?;
    let key = utils::make_key(&signer);

    println!("Using signer {}", signer.address());

    let tokens = hypercore::mainnet().spot_tokens().await?;
    let token = tokens
        .iter()
        .find(|token| token.name == cmd.token)
        .ok_or(anyhow::anyhow!("token {} not found", cmd.token))?;

    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap()
            .tick_strings(CONNECTING_STRINGS),
    );

    let (ticket, gossip, router) = utils::start_gossip(key, true).await?;

    pb.finish_and_clear();

    let topic_id = make_topic(cmd.multi_sig_addr);
    let nonce = NonceHandler::default().next();

    let action = Action::from(
        SendAsset {
            destination: cmd.to,
            source_dex: cmd.source.clone().unwrap_or_default(),
            destination_dex: cmd.dest.clone().unwrap_or_default(),
            token: SendToken(token.clone()),
            amount: cmd.amount,
            from_sub_account: "".to_owned(),
            nonce,
        }
        .into_action(cmd.chain),
    );

    let action = MultiSigPayload {
        multi_sig_user: cmd.multi_sig_addr.to_string().to_lowercase(),
        outer_signer: signer.address().to_string().to_lowercase(),
        action: Box::new(action),
    };

    let mut signatures = vec![];

    let pb = ProgressBar::new(multisig_config.threshold as u64);
    pb.set_style(ProgressStyle::with_template("{msg}\nAuthorized {pos}/{len}").unwrap());

    if multisig_config.authorized_users.contains(&signer.address()) {
        println!("Using current signer {} to sign message", signer.address());
        signatures.push(utils::sign(&signer, nonce, cmd.chain, action.clone()).await?);
        pb.inc(1);
    }

    // just subscribe to the topic
    let mut topic = gossip.subscribe(topic_id, vec![]).await?;

    pb.set_message(format!(
        "Authorized users: {:?}\n\nhypecli multisig sign --multi-sig-addr {} --chain {} --connect {}",
        multisig_config.authorized_users, cmd.multi_sig_addr, cmd.chain, ticket
    ));

    while signatures.len() < multisig_config.threshold {
        tokio::select! {
            _ = ctrl_c() => {
                router.shutdown().await?;
                return Ok(());
            }
            res = topic.next() => {
                match res {
                    Some(Ok(event)) => {
                        match event {
                            Event::NeighborUp(_public_key) => {
                                // println!("Neighbor up: {public_key}");
                                let reply = rmp_serde::to_vec(&Message::Action(nonce, action.clone()))?;
                                topic.broadcast_neighbors(reply.into()).await?;
                            }
                            Event::NeighborDown(_public_key) => {
                                // println!("Neighbor down: {public_key}");
                            }
                            Event::Received(incoming) => {
                                let msg: Message = rmp_serde::from_slice(&incoming.content)?;
                                match msg {
                                    Message::Action(_, _) => {
                                        // ignore
                                    }
                                    Message::Signature(signature) => {
                                        pb.inc(1);
                                        println!("Received: {signature}");
                                        signatures.push(signature);
                                    }
                                }
                            }
                            Event::Lagged => {}
                        }
                    }
                    _ => {
                        pb.finish();
                        panic!("something went wrong: {res:?}");
                    }
                }
            }
        }
    }

    pb.finish_and_clear();

    let action = MultiSigAction {
        signature_chain_id: "0x66eee".to_string(),
        signatures,
        payload: action,
    };

    let req = hypercore::signing::multisig_lead_msg(&signer, action, nonce, None, None, cmd.chain)
        .await?;
    let res = hl.send(req).await?;
    println!("{res:?}");

    router.shutdown().await?;

    Ok(())
}

pub async fn sign(cmd: MultiSigSign) -> anyhow::Result<()> {
    let multisig_config = HttpClient::new(cmd.chain)
        .multi_sig_config(cmd.multi_sig_addr)
        .await?;
    let signer = find_signer(&cmd.common, &multisig_config.authorized_users).await?;
    let key = utils::make_key(&signer);

    println!("Signer found using {}", signer.address());

    let addr = cmd.connect.endpoint_addr();

    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap()
            .tick_strings(CONNECTING_STRINGS),
    );

    let (_ticket, gossip, router) = utils::start_gossip(key, true).await?;

    pb.finish_and_clear();

    let topic_id = make_topic(cmd.multi_sig_addr);

    let mut topic = gossip.subscribe_and_join(topic_id, vec![addr.id]).await?;

    while let Some(Ok(event)) = topic.next().await {
        match event {
            Event::NeighborUp(public_key) => {
                println!("Neighbor up: {public_key}");
            }
            Event::NeighborDown(public_key) => {
                println!("Neighbor down: {public_key}");
            }
            Event::Received(incoming) => {
                let msg: Message = rmp_serde::from_slice(&incoming.content)?;
                match msg {
                    Message::Action(nonce, action) => {
                        println!("{:#?}", action);
                        print!("Accept (y/n)? ");
                        let _ = stdout().flush();
                        let mut input = [0u8; 1];
                        let _ = stdin().read_exact(&mut input);
                        if input[0] == b'y' {
                            let signature = utils::sign(&signer, nonce, cmd.chain, action).await?;
                            let data = rmp_serde::to_vec(&Message::Signature(signature))?;
                            topic.broadcast_neighbors(data.into()).await?;
                        } else {
                            println!("Rejected");
                        }

                        break;
                    }
                    Message::Signature(_) => {
                        // do nothing
                    }
                }
            }
            Event::Lagged => {}
        }
    }

    router.shutdown().await?;

    Ok(())
}
