//! Utility functions for multi-sig operations and gossip networking.
//!
//! This module provides helper functions for:
//! - Creating gossip network topics from multi-sig addresses
//! - Finding and loading signers (private keys, keystores, Ledger)
//! - Starting gossip nodes for peer-to-peer communication
//! - Signing multi-sig actions

use std::{env::home_dir, str::FromStr};

use alloy::signers::{self, Signer, ledger::LedgerSigner};
use hypersdk::{
    Address,
    hypercore::{
        Chain, PrivateKeySigner, Signature, raw::MultiSigPayload, signing::sign_l1_action,
    },
};
use iroh::{
    Endpoint, SecretKey,
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery},
    protocol::Router,
};
use iroh_gossip::{Gossip, TopicId};
use iroh_tickets::endpoint::EndpointTicket;

use crate::multisig::MultiSigCommon;

/// Creates a deterministic gossip topic ID from a multi-sig address.
///
/// This ensures all participants in a multi-sig transaction use the same
/// gossip topic for coordination.
///
/// # Arguments
///
/// * `multi_sig_addr` - The multi-sig wallet address
///
/// # Returns
///
/// A 32-byte TopicId derived from the address (first 20 bytes are the address,
/// remaining bytes are zero-padded).
pub fn make_topic(multi_sig_addr: Address) -> TopicId {
    let mut topic_bytes = [0u8; 32];
    topic_bytes[0..20].copy_from_slice(&multi_sig_addr[..]);
    TopicId::from_bytes(topic_bytes)
}

/// Generates a random secret key for the gossip node.
///
/// Each gossip session uses a fresh ephemeral key rather than deriving
/// from the signer's key for better privacy.
///
/// # Arguments
///
/// * `_signer` - The signer (unused, kept for potential future use)
///
/// # Returns
///
/// A randomly generated SecretKey for the Iroh endpoint.
pub fn make_key(_signer: &impl Signer) -> SecretKey {
    // let public_address = signer.address();
    // let mut address_bytes = [0u8; 32];
    // address_bytes[0..20].copy_from_slice(&public_address[..]);
    // SecretKey::from_bytes(&address_bytes)
    SecretKey::generate(&mut rand::rng())
}

/// Starts a gossip node for peer-to-peer multi-sig coordination.
///
/// Creates an Iroh endpoint with DNS and mDNS discovery, initializes
/// the gossip protocol, and returns the necessary components for
/// communication.
///
/// # Arguments
///
/// * `key` - Secret key for the endpoint
/// * `wait_online` - Whether to wait for the endpoint to be online before returning
///
/// # Returns
///
/// A tuple containing:
/// - `EndpointTicket`: Connection ticket for peers to join
/// - `Gossip`: Gossip protocol instance
/// - `Router`: Protocol router for managing connections
///
/// # Errors
///
/// Returns an error if the endpoint fails to bind or come online.
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

/// Finds and loads a signer from various sources.
///
/// Attempts to load a signer in the following priority order:
/// 1. Private key (if provided via `--private-key`)
/// 2. Foundry keystore (if provided via `--keystore`)
/// 3. Ledger hardware wallet (scans first 10 derivation paths)
///
/// For Ledger devices, the function searches through derivation paths
/// until it finds one that matches an address in `searching_for`.
///
/// # Arguments
///
/// * `cmd` - Common multi-sig command parameters containing credentials
/// * `searching_for` - List of authorized addresses to search for
///
/// # Returns
///
/// A boxed signer that matches one of the authorized addresses.
///
/// # Errors
///
/// Returns an error if:
/// - Private key is invalid
/// - Keystore file not found or password incorrect
/// - No matching Ledger key found in first 10 paths
/// - No signer source provided
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

/// Signs a multi-sig action using the provided signer.
///
/// Handles both EIP-712 typed data signatures and L1 action signatures
/// depending on the action type. For multi-sig actions that support
/// typed data, uses EIP-712 signing. Otherwise, falls back to L1
/// action signing.
///
/// # Arguments
///
/// * `signer` - The signer to use for signing
/// * `nonce` - Transaction nonce
/// * `chain` - Target chain for the action
/// * `action` - Multi-sig payload to sign
///
/// # Returns
///
/// A cryptographic signature over the action.
///
/// # Errors
///
/// Returns an error if signing fails or if the action hash cannot be computed.
pub async fn sign<S: Signer + Send + Sync>(
    signer: &S,
    nonce: u64,
    chain: Chain,
    action: MultiSigPayload,
) -> anyhow::Result<Signature> {
    let multi_sig_user = action.multi_sig_user.parse().unwrap();
    let lead = action.outer_signer.parse().unwrap();

    if let Some(typed_data) = action
        .action
        .typed_data_multisig(multi_sig_user, lead, chain)
    {
        // typed_data.domain = hypercore::types::MULTISIG_MAINNET_EIP712_DOMAIN;
        let sig = signer.sign_dynamic_typed_data(&typed_data).await?;
        Ok(sig.into())
    } else {
        let connection_id = action.action.hash(nonce, None, None)?;
        sign_l1_action(signer, chain, connection_id).await
    }
}
