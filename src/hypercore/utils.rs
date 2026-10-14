//! Utility functions for HyperCore type serialization and signing.
//!
//! This module contains helper functions used by the types module for:
//! - Serialization of addresses, cloids, and U256 values as hex
//! - MessagePack (RMP) hashing for action signatures
//! - EIP-712 typed data generation
//! - Solidity struct definitions for EIP-712 signing

use alloy::{
    dyn_abi::{Eip712Domain, Eip712Types, PropertyDef, Resolver, TypedData},
    primitives::{Address, B256, U256, keccak256},
    sol_types::SolStruct,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::Cloid;
use crate::hypercore::Chain;

const HYPERLIQUID_EIP_PREFIX: &str = "HyperliquidTransaction:";

pub(super) fn serialize_agent_name<S: Serializer>(
    value: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(value.as_deref().unwrap_or_default())
}

/// Enrich the ordinary user-signed schema, preserving its field order.
pub(super) fn multisig_typed_data(
    mut data: TypedData,
    multi_sig_user: Address,
    lead: Address,
) -> TypedData {
    let mut types = Eip712Types::from(&data.resolver);
    let fields = types
        .get_mut(&data.primary_type)
        .expect("known action schema");
    let index = fields
        .iter()
        .position(|field| field.name() == "hyperliquidChain")
        .expect("user-signed actions include hyperliquidChain")
        + 1;
    fields.splice(
        index..index,
        [
            PropertyDef::new_unchecked("address", "payloadMultiSigUser"),
            PropertyDef::new_unchecked("address", "outerSigner"),
        ],
    );
    data.resolver = Resolver::from(types);
    data.message["payloadMultiSigUser"] =
        const_hex::encode_prefixed(multi_sig_user.as_slice()).into();
    data.message["outerSigner"] = const_hex::encode_prefixed(lead.as_slice()).into();
    data
}

/// Serde module for normalized decimal serialization.
///
/// Normalizes decimals by removing trailing zeros before serialization.
/// This matches the Python SDK's `float_to_wire` behavior which uses
/// `Decimal().normalize()` to ensure consistent MessagePack hashing.
///
/// Example: `dec!(10.0)` serializes as `"10"`, not `"10.0"`
pub(super) mod decimal_normalized {
    use std::str::FromStr;

    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer, de};

    pub fn serialize<S>(value: &Decimal, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // normalize() removes trailing zeros: 10.0 -> 10, 0.100 -> 0.1
        let normalized = value.normalize();
        serializer.serialize_str(&normalized.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Decimal::from_str(&s)
            .map(|d| d.normalize())
            .map_err(de::Error::custom)
    }
}

/// [`decimal_normalized`] for an optional decimal. `None` serializes as `null`.
pub(super) mod decimal_normalized_option {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<Decimal>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => super::decimal_normalized::serialize(value, serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Normalized(#[serde(with = "super::decimal_normalized")] Decimal);

        Ok(Option::<Normalized>::deserialize(deserializer)?.map(|Normalized(d)| d))
    }
}

/// Serde module for `OidOrCloid` that ensures the `Right(Cloid)` variant is always
/// serialized as a hex string (consistent across both JSON and MessagePack formats).
///
/// `either::serde_untagged` delegates to `B128`'s default impl, which uses raw bytes
/// in non-human-readable formats like MessagePack. Hyperliquid's server reconstructs
/// the hash from the JSON representation (hex string), so both sides must agree on
/// hex-string encoding for the hash to verify correctly.
pub(super) mod oid_or_cloid {
    use either::Either;
    use serde::{Deserializer, Serializer, de};

    use super::Cloid;

    pub fn serialize<S>(value: &Either<u64, Cloid>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Either::Left(oid) => serializer.serialize_u64(*oid),
            Either::Right(cloid) => serializer.serialize_str(&format!("{:#x}", cloid)),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Either<u64, Cloid>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Either<u64, Cloid>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a u64 oid or a hex string cloid")
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Either::Left(v))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                v.parse::<Cloid>()
                    .map(Either::Right)
                    .map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// Serializes a cloid (B128) as a hex string.
pub(super) fn serialize_cloid_as_hex<S>(value: &Cloid, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{:#x}", value))
}

/// Deserializes a cloid (B128) from a hex string.
pub(super) fn deserialize_cloid_from_hex<'de, D>(deserializer: D) -> Result<Cloid, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse::<Cloid>().map_err(serde::de::Error::custom)
}

/// Returns `true` if the cloid is zero (used for `skip_serializing_if`).
///
/// When the cloid is zero/default, the field is omitted from serialization
/// to match the Python SDK and server-side hashing behavior.
pub(super) fn is_cloid_zero(value: &Cloid) -> bool {
    value.is_zero()
}

/// Serializes a non-zero cloid as a hex string (same as `serialize_cloid_as_hex`).
///
/// This is used in combination with `skip_serializing_if = "is_cloid_zero"` so this
/// function is only called for non-zero cloids.
pub(super) fn serialize_cloid_option<S>(value: &Cloid, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serialize_cloid_as_hex(value, serializer)
}

/// Deserializes a cloid from a hex string, defaulting to zero if absent.
pub(super) fn deserialize_cloid_option<'de, D>(deserializer: D) -> Result<Cloid, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_cloid_from_hex(deserializer)
}

/// Serializes an address as a hex string.
pub(super) fn serialize_address_as_hex<S>(value: &Address, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{:#x}", value))
}

/// Serializes an optional address as a lowercase hex string, or `null` when absent.
pub(super) fn serialize_option_address_as_hex<S>(
    value: &Option<Address>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(address) => serialize_address_as_hex(address, serializer),
        None => serializer.serialize_none(),
    }
}

/// Serializes `(address, value)` pairs with each address as a lowercase hex string.
pub(super) fn serialize_address_pairs_as_hex<S, T>(
    pairs: &[(Address, T)],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    serializer.collect_seq(
        pairs
            .iter()
            .map(|(address, value)| (format!("{address:#x}"), value)),
    )
}

/// [`serialize_address_pairs_as_hex`] for an optional list. Pair it with
/// `skip_serializing_if = "Option::is_none"`.
pub(super) fn serialize_option_address_pairs_as_hex<S, T>(
    pairs: &Option<Vec<(Address, T)>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    match pairs {
        Some(pairs) => serialize_address_pairs_as_hex(pairs, serializer),
        None => serializer.serialize_none(),
    }
}

/// Deserializes an address from a hex string.
pub(super) fn deserialize_address_from_hex<'de, D>(deserializer: D) -> Result<Address, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse::<Address>().map_err(serde::de::Error::custom)
}

/// Serializes a U256 value as a hex string.
pub(super) fn serialize_as_hex<S>(value: &U256, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{:#x}", value))
}

/// Serializes SignersConfig as a JSON string, or "null" if authorized_users is empty.
///
/// When converting a multisig user back to a normal user, the signers field should be "null".
///
/// The string is signed as-is, so it is written exactly as the Python SDK writes it:
/// sorted lowercase addresses and `json.dumps` spacing.
pub(super) fn serialize_signers_as_json<S>(
    value: &super::types::api::SignersConfig,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if value.authorized_users.is_empty() {
        serializer.serialize_str("null")
    } else {
        let mut users: Vec<_> = value
            .authorized_users
            .iter()
            .map(|user| format!("\"{user:#x}\""))
            .collect();
        users.sort();
        serializer.serialize_str(&format!(
            "{{\"authorizedUsers\": [{}], \"threshold\": {}}}",
            users.join(", "),
            value.threshold
        ))
    }
}

pub(super) fn deserialize_signers_as_json<'de, D>(
    deserializer: D,
) -> Result<super::types::api::SignersConfig, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s == "null" {
        Ok(Default::default())
    } else {
        let data = serde_json::from_str(&s).map_err(serde::de::Error::custom)?;
        Ok(data)
    }
}

/// Deserializes a U256 value from a hex string.
pub(super) fn deserialize_from_hex<'de, D>(deserializer: D) -> Result<U256, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let s = s.strip_prefix("0x").unwrap_or(&s);
    U256::from_str_radix(s, 16).map_err(serde::de::Error::custom)
}

/// Computes the RMP (MessagePack) hash of a value for signing.
///
/// This function serializes the value to MessagePack format, appends the nonce,
/// optional vault address, and optional expiry, then computes the Keccak256 hash.
///
/// # Arguments
///
/// * `value` - The value to hash (typically an Action)
/// * `nonce` - The nonce to append
/// * `maybe_vault_address` - Optional vault address for vault trading
/// * `maybe_expires_after` - Optional expiry timestamp in milliseconds
///
/// # Returns
///
/// The Keccak256 hash as a B256, or an error if serialization fails.
pub(super) fn rmp_hash<T: Serialize>(
    value: &T,
    nonce: u64,
    maybe_vault_address: Option<Address>,
    maybe_expires_after: Option<u64>,
) -> Result<B256, rmp_serde::encode::Error> {
    let mut bytes = rmp_serde::to_vec_named(value)?;
    bytes.extend(nonce.to_be_bytes());

    if let Some(vault_address) = maybe_vault_address {
        bytes.push(1);
        bytes.extend(vault_address.as_slice());
    } else {
        bytes.push(0);
    }

    if let Some(expires_after) = maybe_expires_after {
        bytes.push(0);
        bytes.extend(expires_after.to_be_bytes());
    }

    let signature = keccak256(bytes);
    Ok(B256::from(signature))
}

/// Returns the EIP-712 typed data for a message.
///
/// This function creates the TypedData structure required for EIP-712 signing,
/// including the domain, types, and message data.
///
/// # Arguments
///
/// * `msg` - The message to create typed data for
/// * `signature_chain_id` - The request's `signatureChainId`, when `msg` does not carry it
///
/// # Returns
///
/// A TypedData structure ready for EIP-712 signing.
///
/// # Type Parameters
///
/// * `T` - The Solidity struct type that defines the message structure
pub(super) fn get_typed_data<T: SolStruct>(
    msg: &impl Serialize,
    chain: Chain,
    signature_chain_id: Option<&str>,
) -> TypedData {
    let mut resolver = Resolver::from_struct::<T>();
    resolver
        .ingest_string(T::eip712_encode_type())
        .expect("failed to ingest EIP-712 type");

    let mut types = Eip712Types::from(&resolver);
    let agent_type = types.remove(T::NAME).unwrap();

    let msg = serde_json::to_value(msg).unwrap();
    let domain = signature_domain(
        signature_chain_id.or_else(|| msg["signatureChainId"].as_str()),
        chain,
    );

    types.insert(format!("{HYPERLIQUID_EIP_PREFIX}{}", T::NAME), agent_type);

    TypedData {
        domain,
        resolver: Resolver::from(types),
        primary_type: format!("{HYPERLIQUID_EIP_PREFIX}{}", T::NAME),
        message: msg,
    }
}

/// EIP-712 domain for a user-signed action or a multisig envelope.
///
/// Hyperliquid verifies against the chain ID the request names in `signatureChainId`,
/// which does not have to follow the network: the Python SDK signs with `0x66eee` on
/// Mainnet. Without a chain ID, or with one the exchange could not parse either, this
/// falls back to the network's default domain.
pub(super) fn signature_domain(signature_chain_id: Option<&str>, chain: Chain) -> Eip712Domain {
    let default = chain.domain();
    signature_chain_id
        .and_then(|id| id.strip_prefix("0x"))
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .map(|id| Eip712Domain {
            chain_id: Some(U256::from(id)),
            ..default.clone()
        })
        .unwrap_or(default)
}
