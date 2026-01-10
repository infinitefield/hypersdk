use std::{
    io::{Read, Write, stdin, stdout},
    time::Duration,
};

use alloy::signers::Signer;
use clap::{Args, Subcommand};
use futures::StreamExt;
use hypersdk::{
    Address, Decimal,
    hypercore::{
        self, HttpClient, NonceHandler, SendAsset, SendToken, Signature,
        raw::{self, Action, ConvertToMultiSigUser, MultiSigAction, MultiSigPayload},
    },
};
use indicatif::{ProgressBar, ProgressStyle};
use iroh_gossip::api::Event;
use iroh_tickets::endpoint::EndpointTicket;
use serde::{Deserialize, Serialize};
use tokio::signal::ctrl_c;

use crate::{
    SignerArgs,
    utils::{self, find_signer, make_topic},
};

/// Multi-sig commands regardless of your location.
///
/// This commands setups up a peer-to-peer communication
/// to allow for decentralized multi-sig.
#[derive(Subcommand)]
pub enum MultiSigCmd {
    Sign(MultiSigSign),
    SendAsset(MultiSigSendAsset),
    ConvertToNormalUser(MultiSigConvertToNormalUser),
}

impl MultiSigCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            MultiSigCmd::Sign(cmd) => cmd.run().await,
            MultiSigCmd::SendAsset(cmd) => cmd.run().await,
            MultiSigCmd::ConvertToNormalUser(cmd) => cmd.run().await,
        }
    }
}

/// Command to initiate sending an asset via multi-sig.
///
/// This command creates a multi-sig transaction proposal to send assets from
/// a multi-sig wallet. It uses peer-to-peer gossip to coordinate signatures
/// from authorized signers.
#[derive(Args, derive_more::Deref)]
pub struct MultiSigSendAsset {
    #[deref]
    #[command(flatten)]
    pub common: SignerArgs,
    /// Multi-sig wallet address.
    #[arg(long)]
    pub multi_sig_addr: Address,
    /// Destination address.
    #[arg(long)]
    pub to: Address,
    /// Token to send (symbol name, e.g., "USDC", "HYPE").
    #[arg(long)]
    pub token: String,
    /// Amount to send.
    #[arg(long)]
    pub amount: Decimal,
    /// Source DEX. Can be "spot" or a dex name.
    #[arg(long)]
    pub source: Option<String>,
    /// Destination DEX. Can be "spot" or a dex name.
    #[arg(long)]
    pub dest: Option<String>,
}

impl MultiSigSendAsset {
    pub async fn run(self) -> anyhow::Result<()> {
        send_asset(self).await
    }
}

/// Command to sign a multi-sig transaction proposal.
///
/// This command connects to a peer who initiated a multi-sig transaction
/// and signs the proposed action if approved. Uses peer-to-peer gossip
/// for decentralized coordination.
#[derive(Args, derive_more::Deref)]
pub struct MultiSigSign {
    #[deref]
    #[command(flatten)]
    pub common: SignerArgs,
    /// Endpoint ticket to connect to the transaction initiator.
    #[arg(long)]
    pub connect: EndpointTicket,
    /// Multi-sig wallet address.
    #[arg(long)]
    pub multi_sig_addr: Address,
}

impl MultiSigSign {
    pub async fn run(self) -> anyhow::Result<()> {
        sign(self).await
    }
}

/// Messages exchanged over the gossip network during multi-sig coordination.
#[derive(Serialize, Deserialize)]
enum Message {
    /// A proposed action with its nonce that needs to be signed.
    Action(u64, MultiSigPayload),
    /// A signature from an authorized signer.
    Signature(Signature),
}

/// Animation strings for the connecting spinner.
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

async fn send_asset(cmd: MultiSigSendAsset) -> anyhow::Result<()> {
    let hl = HttpClient::new(cmd.chain);
    let multisig_config = hl.multi_sig_config(cmd.multi_sig_addr).await?;
    let signer = find_signer(&cmd.common, Some(&multisig_config.authorized_users)).await?;

    println!("Using signer {}", signer.address());

    let tokens = hypercore::mainnet().spot_tokens().await?;
    let token = tokens
        .iter()
        .find(|token| token.name == cmd.token)
        .ok_or(anyhow::anyhow!("token {} not found", cmd.token))?;

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

    execute_multisig_action(
        cmd.multi_sig_addr,
        hl,
        signer,
        action,
        nonce,
        &multisig_config,
    )
    .await
}

/// Command to convert a multi-signature user back to a normal user.
///
/// This command uses peer-to-peer gossip to collect signatures from authorized
/// signers to convert a multisig account back to a regular single-signer account.
#[derive(Args, derive_more::Deref)]
pub struct MultiSigConvertToNormalUser {
    #[deref]
    #[command(flatten)]
    pub common: SignerArgs,
    /// Multi-sig wallet address.
    #[arg(long)]
    pub multi_sig_addr: Address,
}

impl MultiSigConvertToNormalUser {
    pub async fn run(self) -> anyhow::Result<()> {
        convert_to_normal_user(self).await
    }
}

async fn convert_to_normal_user(cmd: MultiSigConvertToNormalUser) -> anyhow::Result<()> {
    let hl = HttpClient::new(cmd.chain);
    let multisig_config = hl.multi_sig_config(cmd.multi_sig_addr).await?;
    let signer = find_signer(&cmd.common, Some(&multisig_config.authorized_users)).await?;

    println!("Using signer {}", signer.address());
    println!(
        "Converting multisig account {} to normal user",
        cmd.multi_sig_addr
    );

    let nonce = NonceHandler::default().next();

    let action = Action::ConvertToMultiSigUser(ConvertToMultiSigUser {
        signature_chain_id: cmd.chain.arbitrum_id().to_owned(),
        hyperliquid_chain: cmd.chain,
        signers: hypersdk::hypercore::raw::SignersConfig {
            authorized_users: vec![], // Empty to convert to normal user
            threshold: 0,
        },
        nonce,
    });

    execute_multisig_action(
        cmd.multi_sig_addr,
        hl,
        signer,
        action,
        nonce,
        &multisig_config,
    )
    .await
}

async fn sign(cmd: MultiSigSign) -> anyhow::Result<()> {
    let multisig_config = HttpClient::new(cmd.chain)
        .multi_sig_config(cmd.multi_sig_addr)
        .await?;
    let signer = find_signer(&cmd.common, Some(&multisig_config.authorized_users)).await?;
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

    let (_ticket, gossip, router) = utils::start_gossip(key, false).await?;
    // force connect and handle the connection
    let conn = router
        .endpoint()
        .connect(addr.clone(), iroh_gossip::ALPN)
        .await?;
    gossip.handle_connection(conn).await?;

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
                let msg: Message = rmp_serde::from_slice(&incoming.content).map_err(|err| {
                    anyhow::anyhow!("unable to decode content: {err}: {:?}", incoming.content)
                })?;
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
                            topic.broadcast(data.into()).await?;
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

/// Execute a multisig action by collecting signatures from authorized signers.
///
/// This is the core multisig execution logic used by all multisig commands.
async fn execute_multisig_action(
    multi_sig_addr: Address,
    hl: HttpClient,
    signer: Box<dyn Signer + Send + Sync>,
    action: Action,
    nonce: u64,
    multisig_config: &hypersdk::hypercore::MultiSigConfig,
) -> anyhow::Result<()> {
    let key = utils::make_key(&signer);

    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap()
            .tick_strings(CONNECTING_STRINGS),
    );

    let (ticket, gossip, router) = utils::start_gossip(key, true).await?;

    pb.finish_and_clear();

    let topic_id = make_topic(multi_sig_addr);

    let action = MultiSigPayload {
        multi_sig_user: multi_sig_addr.to_string().to_lowercase(),
        outer_signer: signer.address().to_string().to_lowercase(),
        action: Box::new(action),
    };

    let mut signatures = vec![];

    let pb = ProgressBar::new(multisig_config.threshold as u64);
    pb.set_style(ProgressStyle::with_template("{msg}\nAuthorized {pos}/{len}").unwrap());

    if multisig_config.authorized_users.contains(&signer.address()) {
        println!(
            "Using current signer {} to sign message:\n{action:#?}",
            signer.address()
        );
        signatures.push(utils::sign(&signer, nonce, hl.chain(), action.clone()).await?);
        pb.inc(1);
    }

    // Subscribe to the topic
    let mut topic = gossip.subscribe(topic_id, vec![]).await?;

    pb.set_message(format!(
        "Authorized users: {:?}\n\nhypecli multisig sign --multi-sig-addr {} --chain {} --connect {}",
        multisig_config.authorized_users, multi_sig_addr, hl.chain(), ticket
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
                                let reply = rmp_serde::to_vec(&Message::Action(nonce, action.clone()))?;
                                topic.broadcast(reply.into()).await?;
                            }
                            Event::NeighborDown(_public_key) => {
                                // ignore
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

    let multi_sig_action = MultiSigAction {
        signature_chain_id: hl.chain().arbitrum_id().to_owned(),
        signatures,
        payload: action,
    };

    let req = hypercore::signing::multisig_lead_msg(
        &signer,
        multi_sig_action,
        nonce,
        None,
        None,
        hl.chain(),
    )
    .await?;

    match hl.send(req).await? {
        raw::ApiResponse::Ok(_) => {
            println!("Success");
        }
        raw::ApiResponse::Err(err) => {
            println!("error: {err}");
        }
    }

    router.shutdown().await?;

    Ok(())
}
