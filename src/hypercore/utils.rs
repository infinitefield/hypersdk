//! Utility functions for HyperCore type serialization and signing.
//!
//! This module contains helper functions used by the types module for:
//! - Serialization of addresses, cloids, and U256 values as hex
//! - MessagePack (RMP) hashing for action signatures
//! - EIP-712 typed data generation
//! - Solidity struct definitions for EIP-712 signing

use alloy::{
    dyn_abi::{Eip712Types, Resolver, TypedData},
    primitives::{Address, B256, U256, keccak256},
    sol_types::SolStruct,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::Cloid;
use crate::hypercore::Chain;

const HYPERLIQUID_EIP_PREFIX: &str = "HyperliquidTransaction:";

/// Serde module for normalized decimal serialization.
///
/// Normalizes decimals by removing trailing zeros before serialization.
/// This matches the Python SDK's `float_to_wire` behavior which uses
/// `Decimal().normalize()` to ensure consistent MessagePack hashing.
///
/// Example: `dec!(10.0)` serializes as `"10"`, not `"10.0"`
pub(super) mod decimal_normalized {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer, de};
    use std::str::FromStr;

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
                v.parse::<Cloid>().map(Either::Right).map_err(de::Error::custom)
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

/// Serializes an address as a hex string.
pub(super) fn serialize_address_as_hex<S>(value: &Address, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{:#x}", value))
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
        let json = serde_json::to_string(value).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&json)
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
/// * `multi_sig` - Optional multisig information (multisig user address, outer signer address)
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
    multi_sig: Option<(Address, Address)>,
) -> TypedData {
    let mut resolver = Resolver::from_struct::<T>();
    resolver
        .ingest_string(T::eip712_encode_type())
        .expect("failed to ingest EIP-712 type");

    let mut types = Eip712Types::from(&resolver);
    let agent_type = types.remove(T::NAME).unwrap();

    let mut msg = serde_json::to_value(msg).unwrap();
    if let Some((multi_sig_address, lead)) = multi_sig {
        let obj = msg.as_object_mut().unwrap();
        obj.insert(
            "payloadMultiSigUser".into(),
            multi_sig_address.to_string().to_lowercase().into(),
        );
        obj.insert("outerSigner".into(), lead.to_string().to_lowercase().into());
    }

    types.insert(format!("{HYPERLIQUID_EIP_PREFIX}{}", T::NAME), agent_type);

    TypedData {
        domain: chain.domain(),
        resolver: Resolver::from(types),
        primary_type: format!("{HYPERLIQUID_EIP_PREFIX}{}", T::NAME),
        message: msg,
    }
}
