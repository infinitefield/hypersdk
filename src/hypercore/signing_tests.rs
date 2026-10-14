//! Offline interoperability tests against Hyperliquid's Python SDK.
use alloy::{primitives::B256, signers::local::PrivateKeySigner};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::{
    Address, Chain,
    api::{Action, ConvertToMultiSigUser, MultiSigPayload},
    signing::multisig_collect_signatures_with_context,
};

fn fixtures() -> Value {
    serde_json::from_str(include_str!("testdata/signing.json")).unwrap()
}

fn signer() -> PrivateKeySigner {
    "0000000000000000000000000000000000000000000000000000000000000001"
        .parse()
        .unwrap()
}

#[tokio::test]
async fn user_signed_hashes_match_python_sdk() {
    let fixtures = fixtures();
    let signer = signer();
    for case in fixtures["userSigned"].as_array().unwrap() {
        let chain: Chain = case["chain"].as_str().unwrap().parse().unwrap();
        let nonce = case["nonce"].as_u64().unwrap();
        let action: Action = serde_json::from_value(case["action"].clone()).unwrap();
        let expected: B256 = case["prehash"].as_str().unwrap().parse().unwrap();
        assert_eq!(action.prehash(nonce, None, None, chain).unwrap(), expected);
        let ordinary = action
            .clone()
            .sign(&signer, nonce, None, None, chain)
            .await
            .unwrap();
        assert_eq!(ordinary.recover(chain).unwrap(), signer.address());
        let payload = MultiSigPayload {
            multi_sig_user: fixtures["multisigUser"].as_str().unwrap().into(),
            outer_signer: fixtures["leader"].as_str().unwrap().into(),
            action: Box::new(action),
        };
        let expected: B256 = case["multisigPrehash"].as_str().unwrap().parse().unwrap();
        assert_eq!(payload.prehash(nonce, chain).unwrap(), expected);
        let sig = payload.sign(&signer, nonce, chain).await.unwrap();
        assert_eq!(
            sig.to_string(),
            payload
                .sign_sync(&signer, nonce, chain)
                .unwrap()
                .to_string()
        );
        assert_eq!(
            payload.recover(&sig, nonce, chain).unwrap(),
            signer.address()
        );
    }
}

#[tokio::test]
async fn multisig_l1_context_hashes_match_python_sdk() {
    let fixtures = fixtures();
    let signer = signer();
    for case in fixtures["l1Contexts"].as_array().unwrap() {
        let chain = case["chain"].as_str().unwrap().parse().unwrap();
        let nonce = case["nonce"].as_u64().unwrap();
        let vault = case["vault"].as_str().map(|s| s.parse().unwrap());
        let expiry = case["expiresAfter"]
            .as_i64()
            .map(|t| DateTime::<Utc>::from_timestamp_millis(t).unwrap());
        let action = serde_json::from_value(case["action"].clone()).unwrap();
        let multi = multisig_collect_signatures_with_context(
            signer.address(),
            fixtures["multisigUser"].as_str().unwrap().parse().unwrap(),
            std::iter::once(&signer),
            std::iter::empty(),
            action,
            nonce,
            vault,
            expiry,
            chain,
        )
        .await
        .unwrap();
        let expected: B256 = case["prehash"].as_str().unwrap().parse().unwrap();
        assert_eq!(
            multi
                .payload
                .prehash_with_context(nonce, vault, expiry, chain)
                .unwrap(),
            expected
        );
        let sig = &multi.signatures[0];
        assert_eq!(
            multi
                .payload
                .recover_with_context(sig, nonce, vault, expiry, chain)
                .unwrap(),
            signer.address()
        );
        assert_eq!(
            sig.to_string(),
            multi
                .payload
                .sign_sync_with_context(&signer, nonce, vault, expiry, chain)
                .unwrap()
                .to_string()
        );
        if vault.is_some() || expiry.is_some() {
            assert_ne!(
                multi.payload.recover(sig, nonce, chain).unwrap(),
                signer.address()
            );
        }
        let request =
            super::signing::multisig_lead_msg(&signer, multi, nonce, vault, expiry, chain)
                .await
                .unwrap();
        assert_eq!(request.vault_address, vault);
        assert_eq!(
            request.expires_after,
            expiry.map(|t| t.timestamp_millis() as u64)
        );
        assert_eq!(request.recover(chain).unwrap(), signer.address());
    }
}

#[test]
fn unnamed_agent_serializes_and_signs_as_empty_string() {
    let mut case = fixtures()["userSigned"][0]["action"].clone();
    case["agentName"] = Value::Null;
    let action: Action = serde_json::from_value(case).unwrap();
    assert_eq!(serde_json::to_value(&action).unwrap()["agentName"], "");
    let expected: B256 = fixtures()["userSigned"][0]["prehash"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        action
            .prehash(1_700_000_000_000, None, None, Chain::Mainnet)
            .unwrap(),
        expected
    );
}

#[test]
fn all_user_signed_actions_get_multisig_fields() {
    let address = "0x2222222222222222222222222222222222222222";
    let multisig: Address = "0x1111111111111111111111111111111111111111"
        .parse()
        .unwrap();
    let signer = signer();
    let cases = [
        json!({"type":"usdSend", "destination":address,"amount":"10", "time":1}),
        json!({"type":"spotSend", "destination":address,"amount":"10", "token":"USDC:0x1", "time":1}),
        json!({"type":"sendAsset", "destination":address,"amount":"10", "token":"USDC:0x1", "sourceDex":"spot", "destinationDex":"", "fromSubAccount":""}),
        json!({"type":"sendToEvmWithData", "token":"USDC:0x1", "amount":"10", "sourceDex":"spot", "destinationRecipient":address, "addressEncoding":"hex", "destinationChainId":1,"gasLimit":100000,"data":"0x1234"}),
        json!({"type":"approveAgent", "agentAddress":address,"agentName":null}),
        json!({"type":"approveBuilderFee", "builder":address,"maxFeeRate":"0.001%"}),
        json!({"type":"convertToMultiSigUser", "signers":"null"}),
        json!({"type":"userDexAbstraction", "user":address,"enabled":true}),
        json!({"type":"userSetAbstraction", "user":address,"abstraction":"unifiedAccount"}),
        json!({"type":"userPortfolioMargin", "user":address,"enabled":true}),
        json!({"type":"linkStakingUser", "user":address,"isFinalize":false}),
        json!({"type":"stakingLinkDisableTradingUser", "tradingUser":address}),
        json!({"type":"withdraw3", "destination":address,"amount":"10", "time":1}),
        json!({"type":"usdClassTransfer", "amount":"10", "toPerp":true}),
        json!({"type":"tokenDelegate", "validator":address,"wei":1000,"isUndelegate":false}),
    ];
    for mut case in cases {
        case["signatureChainId"] = "0xa4b1".into();
        case["hyperliquidChain"] = "Mainnet".into();
        case["nonce"] = 1.into();
        let action: Action = serde_json::from_value(case.clone()).unwrap();
        let data = action
            .typed_data_multisig(multisig, signer.address(), Chain::Mainnet)
            .unwrap_or_else(|| panic!("missing multisig schema for {}", case["type"]));
        let fields = serde_json::to_value(&data.resolver).unwrap();
        let names: Vec<_> = fields[&data.primary_type]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            &names[..3],
            &["hyperliquidChain", "payloadMultiSigUser", "outerSigner"]
        );
        let payload = MultiSigPayload {
            multi_sig_user: const_hex::encode_prefixed(multisig.as_slice()),
            outer_signer: const_hex::encode_prefixed(signer.address().as_slice()),
            action: Box::new(action),
        };
        let signature = payload.sign_sync(&signer, 1, Chain::Mainnet).unwrap();
        assert_eq!(
            payload.recover(&signature, 1, Chain::Mainnet).unwrap(),
            signer.address()
        );
    }
    assert!(
        Action::Noop
            .typed_data_multisig(multisig, signer.address(), Chain::Mainnet)
            .is_none()
    );
}

#[test]
fn python_default_chain_id_signatures_match_python_sdk() {
    let fixtures = fixtures();
    let signer = signer();
    for case in fixtures["pythonDefaultChainId"].as_array().unwrap() {
        let chain: Chain = case["chain"].as_str().unwrap().parse().unwrap();
        let nonce = case["nonce"].as_u64().unwrap();

        // Built from unsorted, checksummed signers, the string still matches Python's.
        let mut convert: ConvertToMultiSigUser =
            serde_json::from_value(case["convert"].clone()).unwrap();
        convert.signers.authorized_users.reverse();
        assert_eq!(
            serde_json::to_value(&convert).unwrap()["signers"],
            case["convert"]["signers"]
        );
        let convert = Action::ConvertToMultiSigUser(convert);
        let signed = convert
            .sign_sync(&signer, nonce, None, None, chain)
            .unwrap();
        assert_eq!(
            serde_json::to_value(signed.signature).unwrap(),
            case["convertSignature"]
        );

        let Action::MultiSig(multi) = serde_json::from_value(case["multiSig"].clone()).unwrap()
        else {
            panic!("fixture is a multiSig action");
        };
        let participant = multi.payload.sign_sync(&signer, nonce, chain).unwrap();
        assert_eq!(
            serde_json::to_value(participant).unwrap(),
            case["multiSig"]["signatures"][0]
        );
        let request = super::signing::multisig_lead_msg_sync(
            &signer,
            multi.clone(),
            nonce,
            None,
            None,
            chain,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(request.signature).unwrap(),
            case["outerSignature"]
        );
        assert_eq!(request.recover(chain).unwrap(), signer.address());
        let outer = Action::MultiSig(multi)
            .sign_sync(&signer, nonce, None, None, chain)
            .unwrap();
        assert_eq!(
            serde_json::to_value(outer.signature).unwrap(),
            case["outerSignature"]
        );
    }
}
