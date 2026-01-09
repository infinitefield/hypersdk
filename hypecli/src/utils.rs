use std::{env::home_dir, str::FromStr};

use alloy::signers::{self, Signer, ledger::LedgerSigner};
use hypersdk::{
    Address,
    hypercore::{
        self, Chain, PrivateKeySigner, Signature, raw::MultiSigPayload, signing::sign_l1_action,
    },
};
use iroh::{
    Endpoint, SecretKey,
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery},
    protocol::Router,
};
use iroh_gossip::{Gossip, TopicId};
use iroh_tickets::endpoint::EndpointTicket;

use crate::MultiSigCommon;

pub fn make_topic(multi_sig_addr: Address) -> TopicId {
    let mut topic_bytes = [0u8; 32];
    topic_bytes[0..20].copy_from_slice(&multi_sig_addr[..]);
    TopicId::from_bytes(topic_bytes)
}

pub fn make_key(signer: &impl Signer) -> SecretKey {
    let public_address = signer.address();
    let mut address_bytes = [0u8; 32];
    address_bytes[0..20].copy_from_slice(&public_address[..]);
    SecretKey::from_bytes(&address_bytes)
}

pub async fn start_gossip(
    key: iroh::SecretKey,
    wait_online: bool,
) -> anyhow::Result<(EndpointTicket, Gossip, Router)> {
    let endpoint = Endpoint::builder()
        .secret_key(key)
        .relay_mode(iroh::RelayMode::Default)
        .discovery(DnsDiscovery::n0_dns())
        .discovery(MdnsDiscovery::builder().advertise(true))
        .bind()
        .await?;

    let ticket = EndpointTicket::new(endpoint.addr());

    if wait_online {
        let _ = endpoint.online().await;
    }

    let gossip = Gossip::builder().spawn(endpoint.clone());

    let router = Router::builder(endpoint)
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    Ok((ticket, gossip, router))
}

pub async fn find_signer(
    cmd: &MultiSigCommon,
    searching_for: &[Address],
) -> anyhow::Result<Box<dyn Signer + Send + Sync + 'static>> {
    if let Some(key) = cmd.private_key.as_ref() {
        Ok(Box::new(PrivateKeySigner::from_str(key)?) as Box<_>)
    } else if let Some(filename) = cmd.keystore.as_ref() {
        let home_dir = home_dir().ok_or(anyhow::anyhow!("unable to locate home dir"))?;
        let keypath = home_dir.join(".foundry").join("keystores").join(filename);
        let password = cmd
            .password
            .clone()
            .or_else(|| {
                rpassword::prompt_password(format!(
                    "{} password: ",
                    keypath.as_os_str().to_str().unwrap()
                ))
                .ok()
            })
            .ok_or(anyhow::anyhow!("keystores require a password!"))?;
        Ok(Box::new(PrivateKeySigner::decrypt_keystore(keypath, password)?) as Box<_>)
    } else {
        for i in 0..10 {
            if let Ok(ledger) =
                LedgerSigner::new(signers::ledger::HDPath::LedgerLive(i), Some(1)).await
            {
                if searching_for.contains(&ledger.address()) {
                    return Ok(Box::new(ledger) as Box<_>);
                }
            }
        }
        Err(anyhow::anyhow!("unable to find matching key in ledger"))
    }
}

pub async fn sign<S: Signer + Send + Sync>(
    signer: &S,
    nonce: u64,
    chain: Chain,
    action: MultiSigPayload,
) -> anyhow::Result<Signature> {
    let multi_sig_user = action.multi_sig_user.parse().unwrap();
    let lead = action.outer_signer.parse().unwrap();

    if let Some(mut typed_data) = action.action.typed_data_multisig(multi_sig_user, lead) {
        typed_data.domain = hypercore::types::MULTISIG_MAINNET_EIP712_DOMAIN;
        let sig = signer.sign_dynamic_typed_data(&typed_data).await?;
        Ok(sig.into())
    } else {
        let connection_id = action.action.hash(nonce, None, None)?;
        sign_l1_action(signer, chain, connection_id).await
    }
}
