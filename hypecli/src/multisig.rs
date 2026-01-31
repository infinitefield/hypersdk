use std::{
    io::{Write, stdout},
    time::Duration,
};

use alloy::signers::Signer;
use clap::{Args, Subcommand};
use futures::{SinkExt, StreamExt};
use hypersdk::{
    Address, Decimal,
    hypercore::{
        self, AssetTarget, HttpClient, NonceHandler, SendAsset, SendToken, Signature,
        api::{
            self, Action, ConvertToMultiSigUser, MultiSigAction, MultiSigPayload, SignersConfig,
        },
    },
};
use indicatif::{ProgressBar, ProgressStyle};
use iroh::{endpoint::Connection, protocol::Router};
use iroh_tickets::endpoint::EndpointTicket;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, stdin},
    signal::ctrl_c,
    sync::mpsc::unbounded_channel,
};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::{
    SignerArgs,
    utils::{self, find_signer},
};

/// Multi-sig commands regardless of your location.
///
/// This commands setups up a peer-to-peer communication
/// to allow for decentralized multi-sig.
#[derive(Subcommand)]
pub enum MultiSigCmd {
    Sign(MultiSigSign),
    Update(UpdateMultiSigCmd),
    SendAsset(MultiSigSendAsset),
    ConvertToNormalUser(MultiSigConvertToNormalUser),
}

impl MultiSigCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            MultiSigCmd::Sign(cmd) => cmd.run().await,
            MultiSigCmd::SendAsset(cmd) => cmd.run().await,
            MultiSigCmd::ConvertToNormalUser(cmd) => cmd.run().await,
            MultiSigCmd::Update(cmd) => cmd.run().await,
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

/// Update the multi-sig user.
#[derive(Args, derive_more::Deref)]
pub struct UpdateMultiSigCmd {
    #[deref]
    #[command(flatten)]
    common: SignerArgs,

    /// Authorized signer addresses (comma-separated)
    #[arg(long, required = true)]
    authorized_user: Vec<Address>,

    /// Signature threshold (number of signatures required)
    #[arg(long)]
    threshold: usize,

    /// Multi-sig wallet address.
    #[arg(long)]
    multi_sig_addr: Address,
}

impl UpdateMultiSigCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        update(self).await
    }
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

    let source_dex: AssetTarget = cmd
        .source
        .as_ref()
        .map(|s| s.parse().unwrap_or(AssetTarget::Perp))
        .unwrap_or(AssetTarget::Perp);

    let destination_dex: AssetTarget = cmd
        .dest
        .as_ref()
        .map(|s| s.parse().unwrap_or(AssetTarget::Perp))
        .unwrap_or(AssetTarget::Perp);

    let action = Action::from(
        SendAsset {
            destination: cmd.to,
            source_dex,
            destination_dex,
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

async fn update(cmd: UpdateMultiSigCmd) -> anyhow::Result<()> {
    let hl = HttpClient::new(cmd.chain);
    let multisig_config = hl.multi_sig_config(cmd.multi_sig_addr).await?;
    let signer = find_signer(&cmd.common, Some(&multisig_config.authorized_users)).await?;

    println!("Using signer {}", signer.address());

    let nonce = NonceHandler::default().next();

    let signature_chain_id = hl.chain().arbitrum_id().to_owned();
    let action = Action::from(ConvertToMultiSigUser {
        signature_chain_id,
        hyperliquid_chain: hl.chain(),
        signers: SignersConfig {
            authorized_users: cmd.authorized_user,
            threshold: cmd.threshold,
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
        signers: hypersdk::hypercore::api::SignersConfig {
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

    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap()
            .tick_strings(CONNECTING_STRINGS),
    );

    let (endpoint, _ticket) = utils::start_gossip(key, true).await?;

    let addr = cmd.connect.endpoint_addr();
    // force connect and handle the connection
    let conn = endpoint.connect(addr.clone(), proto::ALPN).await?;

    pb.finish_and_clear();

    let (send, recv) = conn.open_bi().await?;

    let mut read = FramedRead::new(recv, proto::Codec::default());
    let mut write = FramedWrite::new(send, proto::Codec::default());

    let _ = write.send(proto::Message::Hello).await;

    match read.next().await {
        Some(Ok(proto::Message::Action(nonce, action))) => {
            println!("{:#?}", action);
            print!("Accept (y/n)? ");
            let _ = stdout().flush();
            let mut input = [0u8; 1];
            let _ = stdin().read_exact(&mut input).await;
            if input[0] == b'y' {
                let signature = action.sign(&signer, nonce, cmd.chain).await?;
                write.send(proto::Message::Signature(signature)).await?;
            } else {
                println!("Rejected");
            }
        }
        _ => {
            panic!("unexpected message");
        }
    }

    conn.closed().await;
    endpoint.close().await;

    Ok(())
}

/// Execute a multisig action by collecting signatures from authorized signers.
///
/// This is the core multisig execution logic used by all multisig commands.
async fn execute_multisig_action(
    multi_sig_addr: Address,
    hl: HttpClient,
    signer: Box<dyn Signer + Send + Sync>,
    inner_action: Action,
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

    let (endpoint, ticket) = utils::start_gossip(key, true).await?;

    pb.finish_and_clear();

    let action = MultiSigPayload {
        multi_sig_user: multi_sig_addr.to_string().to_lowercase(),
        outer_signer: signer.address().to_string().to_lowercase(),
        action: Box::new(inner_action),
    };

    let mut signatures = vec![];

    let pb = ProgressBar::new(multisig_config.threshold as u64);
    pb.set_style(ProgressStyle::with_template("{msg}\nAuthorized {pos}/{len}").unwrap());

    if multisig_config.authorized_users.contains(&signer.address()) {
        println!(
            "Using current signer {} to sign message:\n{action:#?}",
            signer.address()
        );
        signatures.push(action.sign(&signer, nonce, hl.chain()).await?);
        pb.inc(1);
    }

    let (tx, mut rx) = unbounded_channel();
    let router = Router::builder(endpoint)
        .accept(
            proto::ALPN,
            proto::Serve((nonce, action.clone(), tx.clone())),
        )
        .spawn();

    let mut msgs = String::new();

    use std::fmt::Write;

    while signatures.len() < multisig_config.threshold {
        pb.set_message(format!(
            "Authorized users: {:?}\n{msgs}\nhypecli multisig sign --multi-sig-addr {} --chain {} --connect {}",
            multisig_config.authorized_users, multi_sig_addr, hl.chain(), ticket
        ));

        tokio::select! {
            _ = ctrl_c() => {
                router.shutdown().await?;
                return Ok(());
            }
            Some(signature) = rx.recv() => {
                writeln!(&mut msgs, "> Receive signature {signature}")?;
                match action.recover(&signature, nonce, hl.chain()) {
                    Ok(address) => {
                        if !multisig_config.authorized_users.contains(&address) {
                            writeln!(&mut msgs, ">X Received signature from unauthorized user {address}")?;
                        } else {
                            pb.inc(1);
                            writeln!(&mut msgs, "> Received: {signature}")?;
                            signatures.push(signature);
                        }
                    }
                    Err(err) => {
                        let _ = writeln!(&mut msgs, ">X unable to verify signature: {err}");
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
        api::Response::Ok(_) => {
            println!("Success");
        }
        api::Response::Err(err) => {
            println!("error: {err}");
        }
    }

    router.shutdown().await?;

    Ok(())
}

mod proto {
    use super::*;
    use bytes::{Bytes, BytesMut};
    use futures::SinkExt;
    use iroh::protocol::ProtocolHandler;
    use tokio::sync::mpsc::UnboundedSender;
    use tokio_util::codec::{self, LengthDelimitedCodec};

    pub const ALPN: &[u8] = b"/hypersdk-multisig/0";

    /// Messages exchanged over the gossip network during multi-sig coordination.
    #[derive(Serialize, Deserialize)]
    pub enum Message {
        /// We need to write something when opening the connection
        ///
        /// https://docs.rs/iroh/latest/iroh/endpoint/struct.Connection.html#method.accept_bi
        Hello,
        /// A proposed action with its nonce that needs to be signed.
        Action(u64, MultiSigPayload),
        /// A signature from an authorized signer.
        Signature(Signature),
    }

    #[derive(Default)]
    pub struct Codec {
        inner: LengthDelimitedCodec,
    }

    impl codec::Decoder for Codec {
        type Item = Message;
        type Error = anyhow::Error;

        fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
            let payload = match self.inner.decode(src)? {
                Some(data) => data,
                None => {
                    return Ok(None);
                }
            };

            let msg = rmp_serde::from_slice(&payload)?;
            Ok(Some(msg))
        }
    }

    impl codec::Encoder<Message> for Codec {
        type Error = anyhow::Error;

        fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
            let msg = rmp_serde::to_vec(&item)?;
            self.inner.encode(Bytes::from(msg), dst)?;
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    pub struct Serve(pub (u64, MultiSigPayload, UnboundedSender<Signature>));

    impl ProtocolHandler for Serve {
        fn accept(
            &self,
            connection: Connection,
        ) -> impl Future<Output = Result<(), iroh::protocol::AcceptError>> + Send {
            let (nonce, action, tx) = self.clone().0;
            async move {
                let (send, recv) = connection.accept_bi().await?;

                let mut read = FramedRead::new(recv, proto::Codec::default());
                let mut write = FramedWrite::new(send, proto::Codec::default());

                let _ = write.send(Message::Action(nonce, action)).await;
                loop {
                    match read.next().await {
                        Some(Ok(Message::Signature(sig))) => {
                            let _ = tx.send(sig);
                            break Ok(());
                        }
                        // just read the Hello
                        Some(Ok(Message::Hello)) => {}
                        _ => {
                            println!("received unexpected msg");
                        }
                    }
                }
            }
        }
    }
}
