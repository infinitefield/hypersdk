//! Signing utilities for HyperCore actions.
//!
//! This module provides functions for signing various types of actions on Hyperliquid,
//! including regular actions, multisig actions, and EIP-712 typed data.

use alloy::{
    dyn_abi::TypedData,
    primitives::{Address, B256},
    signers::{Signer, SignerSync},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::hypercore::{
    Chain, MAINNET_MULTISIG_CHAIN_ID,
    types::{
        Action, ActionRequest, CORE_MAINNET_EIP712_DOMAIN, MultiSigAction, MultiSigPayload,
        Signature, get_typed_data, rmp_hash, solidity,
    },
};

/// Send a signed action hashing with typed data.
pub(super) fn sign_eip712<S: SignerSync>(
    signer: &S,
    action: Action,
    typed_data: TypedData,
    nonce: u64,
) -> Result<ActionRequest> {
    let signature = signer.sign_dynamic_typed_data_sync(&typed_data)?;
    let sig: Signature = signature.into();

    Ok(ActionRequest {
        signature: sig,
        action,
        nonce,
        vault_address: None,
        expires_after: None,
    })
}

/// Signs an action using RMP (messagepack) hashing.
pub(super) fn sign_rmp<S: SignerSync>(
    signer: &S,
    action: Action,
    nonce: u64,
    maybe_vault_address: Option<Address>,
    maybe_expires_after: Option<DateTime<Utc>>,
    chain: Chain,
) -> Result<ActionRequest> {
    let expires_after = maybe_expires_after.map(|after| after.timestamp_millis() as u64);
    let connection_id = action.hash(nonce, maybe_vault_address, expires_after)?;

    let signature = sign_l1_action(signer, chain, connection_id)?;

    Ok(ActionRequest {
        signature,
        action,
        nonce,
        vault_address: maybe_vault_address,
        expires_after,
    })
}

/// Signs an L1 action with EIP-712.
#[inline(always)]
pub(super) fn sign_l1_action<S: SignerSync>(
    signer: &S,
    chain: Chain,
    connection_id: B256,
) -> anyhow::Result<Signature> {
    let sig = signer.sign_typed_data_sync(
        &solidity::Agent {
            source: if chain.is_mainnet() { "a" } else { "b" }.to_string(),
            connectionId: connection_id,
        },
        &CORE_MAINNET_EIP712_DOMAIN,
    )?;
    Ok(sig.into())
}

/// Signs a multisig action for submission to the exchange.
///
/// This function creates the final signature that wraps all the collected multisig signatures.
/// The lead signer signs an envelope containing:
/// - The chain identifier (mainnet/testnet)
/// - The hash of the entire multisig action (including all signer signatures)
/// - The nonce
///
/// # EIP-712 Domain
///
/// Always uses the mainnet multisig domain (chainId 0x66eee) for both mainnet and testnet.
/// The `hyperliquidChain` field in the message distinguishes between mainnet and testnet.
///
/// # Parameters
///
/// - `signer`: The lead signer who submits the transaction
/// - `chain`: The chain (mainnet/testnet) - determines the hyperliquidChain field
/// - `action`: The complete multisig action with all collected signatures
/// - `nonce`: Unique transaction nonce
/// - `maybe_vault_address`: Optional vault address if trading on behalf of a vault
/// - `maybe_expires_after`: Optional expiration time for the request
pub(super) fn sign_rmp_multisig<S: SignerSync>(
    signer: &S,
    action: MultiSigAction,
    nonce: u64,
    maybe_vault_address: Option<Address>,
    maybe_expires_after: Option<DateTime<Utc>>,
    chain: Chain,
) -> Result<ActionRequest> {
    let expires_after = maybe_expires_after.map(|after| after.timestamp_millis() as u64);
    let multsig_hash = rmp_hash(&action, nonce, maybe_vault_address, expires_after)?;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Envelope {
        hyperliquid_chain: String,
        multi_sig_action_hash: String,
        nonce: u64,
    }

    let envelope = Envelope {
        hyperliquid_chain: chain.to_string(),
        multi_sig_action_hash: multsig_hash.to_string(),
        nonce,
    };

    // Always use the mainnet multisig domain (chainId 0x66eee) for both mainnet and testnet
    // The hyperliquidChain field in the message distinguishes between mainnet and testnet
    let mut typed_data = get_typed_data::<solidity::SendMultiSig>(&envelope);
    typed_data.domain = super::types::MULTISIG_MAINNET_EIP712_DOMAIN;

    let sig = signer.sign_dynamic_typed_data_sync(&typed_data)?;

    Ok(ActionRequest {
        signature: sig.into(),
        action: Action::MultiSig(action),
        nonce,
        vault_address: maybe_vault_address,
        expires_after,
    })
}

/// Collects signatures from all signers for a multisig action.
///
/// This function implements the Hyperliquid multisig signature collection protocol:
///
/// 1. Creates an action hash from: `[multisig_user, lead_signer, action]`
/// 2. Each signer signs the action hash using EIP-712 with the L1 Agent domain
/// 3. All signatures are collected and packaged into a `MultiSigAction`
///
/// # Address Normalization
///
/// Both the multisig user address and lead signer address are normalized to lowercase
/// before hashing. This ensures consistency across different address representations.
///
/// # Signature Collection
///
/// Each signer must sign the same action hash. The order of signatures typically matches
/// the order of signers in the multisig wallet configuration, though the exact requirements
/// depend on the wallet's setup on Hyperliquid.
///
/// # Returns
///
/// A `MultiSigAction` containing:
/// - All collected signatures
/// - The signature chain ID (always uses mainnet multisig chain ID)
/// - The payload with multisig address, lead signer, and the action
///
/// # Reference
///
/// Based on: https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/be7523d58297a93d0e938063460c14ae45e9034f/hyperliquid/utils/signing.py#L293
pub(super) fn multisig_collect_signatures<'a, S: SignerSync + Signer + 'a>(
    lead: Address,
    multi_sig_user: Address,
    signers: impl Iterator<Item = &'a S>,
    inner_action: Action,
    nonce: u64,
    chain: Chain,
) -> Result<MultiSigAction> {
    // Collect signatures from all signers
    let mut signatures = vec![];

    // Normalize addresses to lowercase for consistent hashing
    let lead = lead.to_string().to_lowercase();
    let multi_sig_user = multi_sig_user.to_string().to_lowercase();

    // Hash the envelope: [multi_sig_user (lowercase), outer_signer (lowercase), action]
    // This hash is what each signer will sign with their private key
    let action_hash = rmp_hash(&(&multi_sig_user, &lead, &inner_action), nonce, None, None)?;

    // Collect a signature from each signer for the action hash
    for signer in signers {
        // TODO: abstract so we can support TypedData
        let sig = sign_l1_action(signer, chain, action_hash)?;
        signatures.push(sig);
    }

    Ok(MultiSigAction {
        signature_chain_id: MAINNET_MULTISIG_CHAIN_ID,
        signatures,
        payload: MultiSigPayload {
            multi_sig_user,
            outer_signer: lead,
            action: Box::new(inner_action),
        },
    })
}

#[cfg(test)]
mod tests {

    use alloy::signers::local::PrivateKeySigner;

    use super::*;
    use crate::hypercore::{
        ARBITRUM_SIGNATURE_CHAIN_ID,
        types::{self, HyperliquidChain},
    };

    fn get_signer() -> PrivateKeySigner {
        let priv_key = "e908f86dbb4d55ac876378565aafeabc187f6690f046459397b17d9b9a19688e";
        priv_key.parse::<PrivateKeySigner>().unwrap()
    }

    #[test]
    fn test_sign_usd_transfer_action() {
        let signer = get_signer();

        let usd_send = types::UsdSend {
            signature_chain_id: ARBITRUM_SIGNATURE_CHAIN_ID,
            hyperliquid_chain: HyperliquidChain::Mainnet,
            destination: "0x0D1d9635D0640821d15e323ac8AdADfA9c111414"
                .parse()
                .unwrap(),
            amount: rust_decimal::Decimal::ONE,
            time: 1690393044548,
        };
        let typed_data = usd_send.typed_data(&usd_send);
        let signature = signer.sign_dynamic_typed_data_sync(&typed_data).unwrap();

        let expected_sig = "0xeca6267bcaadc4c0ae1aed73f5a2c45fcdbb7271f2e9356992404e5d4bad75a3572e08fe93f17755abadb7f84be7d1e9c4ce48bb5633e339bc430c672d5a20ed1b";
        assert_eq!(signature.to_string(), expected_sig);
    }
}
