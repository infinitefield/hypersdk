//! Signing utilities for HyperCore actions.
//!
//! This module provides functions for signing various types of actions on Hyperliquid,
//! including regular actions, multisig actions, and EIP-712 typed data.
//!
//! All signing is done through the `Action` enum, which has `sign_sync`, `sign`,
//! `prehash`, and `recover` methods. Individual action types can be converted to
//! `Action` using `Into`.

use alloy::{
    dyn_abi::TypedData,
    primitives::{Address, B256},
    signers::{Signer, SignerSync},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::hypercore::{
    Chain,
    api::{Action, MultiSigAction, MultiSigPayload},
    types::{CORE_MAINNET_EIP712_DOMAIN, Signature, solidity},
    utils::{get_typed_data, rmp_hash},
};

/// Computes the EIP-712 signing hash for an Agent struct with the given connection ID.
///
/// This is used for RMP-based actions where the signature is over an Agent wrapper
/// containing the RMP hash as the connection ID.
#[inline(always)]
pub fn agent_signing_hash(chain: Chain, connection_id: B256) -> B256 {
    use alloy::sol_types::SolStruct;
    let agent = solidity::Agent {
        source: if chain.is_mainnet() { "a" } else { "b" }.to_string(),
        connectionId: connection_id,
    };
    agent.eip712_signing_hash(&CORE_MAINNET_EIP712_DOMAIN)
}

/// Signs an L1 action with EIP-712 (asynchronous version).
#[inline(always)]
pub async fn sign_l1_action<S: Signer + Send + Sync>(
    signer: &S,
    chain: Chain,
    connection_id: B256,
) -> anyhow::Result<Signature> {
    let sig = signer
        .sign_typed_data(
            &solidity::Agent {
                source: if chain.is_mainnet() { "a" } else { "b" }.to_string(),
                connectionId: connection_id,
            },
            &CORE_MAINNET_EIP712_DOMAIN,
        )
        .await?;
    Ok(sig.into())
}

/// Signs a multisig action for submission to the exchange (synchronous).
///
/// This function creates the final signature that wraps all the collected multisig signatures.
pub fn multisig_lead_msg_sync<S: SignerSync>(
    signer: &S,
    action: MultiSigAction,
    nonce: u64,
    maybe_vault_address: Option<Address>,
    maybe_expires_after: Option<DateTime<Utc>>,
    chain: Chain,
) -> Result<crate::hypercore::api::ActionRequest> {
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

    let typed_data = get_typed_data::<solidity::SendMultiSig>(&envelope, chain, None);
    let sig = signer.sign_dynamic_typed_data_sync(&typed_data)?.into();

    Ok(crate::hypercore::api::ActionRequest {
        signature: sig,
        action: Action::MultiSig(action),
        nonce,
        vault_address: maybe_vault_address,
        expires_after,
    })
}

/// Signs a multisig action for submission to the exchange (asynchronous).
///
/// This function creates the final signature that wraps all the collected multisig signatures.
pub async fn multisig_lead_msg<S: Signer + Send + Sync>(
    signer: &S,
    action: MultiSigAction,
    nonce: u64,
    maybe_vault_address: Option<Address>,
    maybe_expires_after: Option<DateTime<Utc>>,
    chain: Chain,
) -> Result<crate::hypercore::api::ActionRequest> {
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

    let typed_data = get_typed_data::<solidity::SendMultiSig>(&envelope, chain, None);
    let sig = signer.sign_dynamic_typed_data(&typed_data).await?.into();

    Ok(crate::hypercore::api::ActionRequest {
        signature: sig,
        action: Action::MultiSig(action),
        nonce,
        vault_address: maybe_vault_address,
        expires_after,
    })
}

/// Collects signatures from all signers for a multisig action, with support for appending pre-existing signatures.
///
/// This function implements the Hyperliquid multisig signature collection protocol.
pub async fn multisig_collect_signatures<'a, S: Signer + Send + Sync + 'a>(
    lead: Address,
    multi_sig_user: Address,
    signers: impl Iterator<Item = &'a S>,
    signed: impl Iterator<Item = Signature>,
    inner_action: Action,
    nonce: u64,
    chain: Chain,
) -> Result<MultiSigAction> {
    // Normalize addresses (required for consistent hashing)
    let multi_sig_user_str = multi_sig_user.to_string().to_lowercase();
    let lead_str = lead.to_string().to_lowercase();

    // Dispatch to specialized function based on action type
    let mut signatures =
        if let Some(typed_data) = inner_action.typed_data_multisig(multi_sig_user, lead, chain) {
            // EIP-712 typed data actions (UsdSend, SpotSend, SendAsset)
            multisig_collect_eip712_signatures(signers, typed_data).await?
        } else {
            // RMP-based actions (orders, cancels, modifications)
            multisig_collect_rmp_signatures(
                signers,
                &multi_sig_user_str,
                &lead_str,
                &inner_action,
                nonce,
                chain,
            )
            .await?
        };
    signatures.extend(signed);

    Ok(MultiSigAction {
        signature_chain_id: chain.arbitrum_id().to_owned(),
        signatures,
        payload: MultiSigPayload {
            multi_sig_user: multi_sig_user_str,
            outer_signer: lead_str,
            action: Box::new(inner_action),
        },
    })
}

/// Collects signatures for EIP-712 typed data actions (transfers).
async fn multisig_collect_eip712_signatures<'a, S: Signer + Send + Sync + 'a>(
    signers: impl Iterator<Item = &'a S>,
    typed_data: TypedData,
) -> Result<Vec<Signature>> {
    let mut signatures = vec![];
    for signer in signers {
        let signature = signer.sign_dynamic_typed_data(&typed_data).await?;
        signatures.push(signature.into());
    }

    Ok(signatures)
}

/// Collects signatures for RMP-based actions (orders, cancels, modifications).
async fn multisig_collect_rmp_signatures<'a, S: Signer + Send + Sync + 'a>(
    signers: impl Iterator<Item = &'a S>,
    multi_sig_user: &str,
    lead: &str,
    action: &Action,
    nonce: u64,
    chain: Chain,
) -> Result<Vec<Signature>> {
    // Create the RMP hash once
    let connection_id = rmp_hash(&(multi_sig_user, lead, action), nonce, None, None)?;

    let mut signatures = vec![];
    for signer in signers {
        let signature = sign_l1_action(signer, chain, connection_id).await?;
        signatures.push(signature);
    }

    Ok(signatures)
}

#[cfg(test)]
mod tests {
    use alloy::signers::local::PrivateKeySigner;

    use super::*;
    use crate::hypercore::{ARBITRUM_MAINNET_CHAIN_ID, types};

    fn get_signer() -> PrivateKeySigner {
        let priv_key = "e908f86dbb4d55ac876378565aafeabc187f6690f046459397b17d9b9a19688e";
        priv_key.parse::<PrivateKeySigner>().unwrap()
    }

    #[test]
    fn test_sign_usd_transfer_action() {
        let signer = get_signer();

        let usd_send = types::api::UsdSendAction {
            signature_chain_id: ARBITRUM_MAINNET_CHAIN_ID.to_owned(),
            hyperliquid_chain: Chain::Mainnet,
            destination: "0x0D1d9635D0640821d15e323ac8AdADfA9c111414"
                .parse()
                .unwrap(),
            amount: rust_decimal::Decimal::ONE,
            time: 1690393044548,
        };

        let action = Action::UsdSend(usd_send);
        let nonce = 1690393044548u64;
        let signed = action
            .sign_sync(&signer, nonce, None, None, Chain::Mainnet)
            .unwrap();

        let expected_sig = "0xeca6267bcaadc4c0ae1aed73f5a2c45fcdbb7271f2e9356992404e5d4bad75a3572e08fe93f17755abadb7f84be7d1e9c4ce48bb5633e339bc430c672d5a20ed1b";
        assert_eq!(signed.signature.to_string(), expected_sig);
    }

    #[test]
    fn test_recover_usd_send() {
        let signer = get_signer();
        let expected_address = signer.address();

        let usd_send = types::api::UsdSendAction {
            signature_chain_id: ARBITRUM_MAINNET_CHAIN_ID.to_owned(),
            hyperliquid_chain: Chain::Mainnet,
            destination: "0x0D1d9635D0640821d15e323ac8AdADfA9c111414"
                .parse()
                .unwrap(),
            amount: rust_decimal::Decimal::ONE,
            time: 1690393044548,
        };

        let action = Action::UsdSend(usd_send.clone());
        let nonce = 1690393044548u64;
        let action_request = action
            .sign_sync(&signer, nonce, None, None, Chain::Mainnet)
            .unwrap();

        // Recover the address from the signature
        let recovered_address = Action::UsdSend(usd_send)
            .recover(&action_request.signature, nonce, None, None, Chain::Mainnet)
            .unwrap();

        assert_eq!(
            recovered_address, expected_address,
            "Recovered address should match the signer's address"
        );
    }

    #[test]
    fn test_recover_batch_order() {
        use rust_decimal::dec;
        use types::{BatchOrder, OrderGrouping, OrderRequest, OrderTypePlacement, TimeInForce};

        let signer = get_signer();
        let expected_address = signer.address();

        let order = BatchOrder {
            orders: vec![OrderRequest {
                asset: 0,
                is_buy: true,
                limit_px: dec!(50000),
                sz: dec!(0.1),
                reduce_only: false,
                order_type: OrderTypePlacement::Limit {
                    tif: TimeInForce::Gtc,
                },
                cloid: Default::default(),
            }],
            grouping: OrderGrouping::Na,
            builder: None,
        };

        let action = Action::Order(order.clone());
        let nonce = chrono::Utc::now().timestamp_millis() as u64;
        let action_request = action
            .sign_sync(&signer, nonce, None, None, Chain::Mainnet)
            .unwrap();

        // Recover the address from the signature
        let recovered_address = Action::Order(order)
            .recover(&action_request.signature, nonce, None, None, Chain::Mainnet)
            .unwrap();

        assert_eq!(
            recovered_address, expected_address,
            "Recovered address should match the signer's address for RMP-based action"
        );
    }
}
