//! Utility functions for multi-sig operations and gossip networking.
//!
//! This module provides helper functions for:
//! - Creating gossip network topics from multi-sig addresses
//! - Finding and loading signers (private keys, keystores, Ledger)
//! - Starting gossip nodes for peer-to-peer communication
//! - Signing multi-sig actions

use std::{env::home_dir, str::FromStr};

use alloy::signers::{self, Signer, ledger::LedgerSigner};
use anyhow::Context;
use hypersdk::{Address, hypercore::PrivateKeySigner};
use iroh::{
    Endpoint, SecretKey,
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery},
};
use iroh_tickets::endpoint::EndpointTicket;

use crate::SignerArgs;

/// Generates a random secret key for the gossip node.
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
) -> anyhow::Result<(Endpoint, EndpointTicket)> {
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

    Ok((endpoint, ticket))
}

/// Finds and loads a synchronous signer (private key or keystore only).
///
/// This is for operations that require `SignerSync` trait, such as `send_asset`.
/// Ledger hardware wallets are not supported for sync operations.
///
/// # Arguments
///
/// * `cmd` - Command parameters containing credentials
///
/// # Returns
///
/// A `PrivateKeySigner` that implements `SignerSync`.
///
/// # Errors
///
/// Returns an error if:
/// - Private key is invalid
/// - Keystore file not found or password incorrect
/// - No private key or keystore provided
pub fn find_signer_sync(cmd: &SignerArgs) -> anyhow::Result<PrivateKeySigner> {
    if let Some(key) = cmd.private_key.as_ref() {
        Ok(PrivateKeySigner::from_str(key)?)
    } else if let Some(filename) = cmd.keystore.as_ref() {
        let home_dir = home_dir().ok_or(anyhow::anyhow!("unable to locate home dir"))?;
        let keypath = home_dir.join(".foundry").join("keystores").join(filename);
        anyhow::ensure!(keypath.exists(), "keystore {filename} doesn't exist");
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
        PrivateKeySigner::decrypt_keystore(keypath, password).context("decrypt_keystore")
    } else {
        Err(anyhow::anyhow!(
            "This operation requires a private key or keystore (Ledger not supported)"
        ))
    }
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
    cmd: &SignerArgs,
    filter_by: Option<&[Address]>,
) -> anyhow::Result<Box<dyn Signer + Send + Sync + 'static>> {
    if let Some(key) = cmd.private_key.as_ref() {
        Ok(Box::new(PrivateKeySigner::from_str(key)?) as Box<_>)
    } else if let Some(filename) = cmd.keystore.as_ref() {
        let home_dir = home_dir().ok_or(anyhow::anyhow!("unable to locate home dir"))?;
        let keypath = home_dir.join(".foundry").join("keystores").join(filename);
        anyhow::ensure!(keypath.exists(), "keystore {filename} doesn't exist");
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
        Ok(Box::new(
            PrivateKeySigner::decrypt_keystore(keypath, password).context("decrypt_keystore")?,
        ) as Box<_>)
    } else {
        for i in 0..10 {
            if let Ok(ledger) =
                LedgerSigner::new(signers::ledger::HDPath::LedgerLive(i), Some(1)).await
            {
                if let Some(filter_by) = filter_by {
                    if filter_by.contains(&ledger.address()) {
                        return Ok(Box::new(ledger) as Box<_>);
                    }
                } else {
                    return Ok(Box::new(ledger) as Box<_>);
                }
            }
        }
        Err(anyhow::anyhow!("unable to find matching key in ledger"))
    }
}
