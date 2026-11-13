//! EIP-712 streaming over the published Trezor client's raw message API.

use alloy::{
    dyn_abi::{DynSolType, DynSolValue, TypedData, eip712::Resolver},
    primitives::Signature,
};
use anyhow::{Context, bail, ensure};
use trezor_client::{
    client::Trezor,
    protos::{
        self, MessageType,
        ethereum_typed_data_struct_ack::{
            EthereumDataType, EthereumFieldType, EthereumStructMember,
        },
    },
};

pub(super) struct TypedDataStream {
    resolver: Resolver,
    primary_type: String,
    domain: DynSolValue,
    message: DynSolValue,
}

impl TypedDataStream {
    pub(super) fn new(data: &TypedData) -> anyhow::Result<Self> {
        let domain_type = data.domain.encode_type();
        if data.primary_type != "EIP712Domain" {
            for definition in data.resolver.linearize(&data.primary_type)? {
                ensure!(
                    definition.type_name() != "EIP712Domain"
                        || definition.eip712_encode_type() == domain_type,
                    "message references a conflicting EIP712Domain definition"
                );
            }
        }
        let mut resolver = data.resolver.clone();
        resolver.ingest_string(domain_type)?;
        let domain = resolver
            .resolve("EIP712Domain")?
            .coerce_json(&serde_json::to_value(&data.domain)?)?;
        let message = if data.primary_type == "EIP712Domain" {
            domain.clone()
        } else {
            data.coerce()?
        };
        Ok(Self {
            resolver,
            primary_type: data.primary_type.clone(),
            domain,
            message,
        })
    }

    pub(super) fn sign(&self, client: &mut Trezor, path: Vec<u32>) -> anyhow::Result<Signature> {
        let result = self.sign_inner(client, path);
        if result.is_err() {
            // End an interrupted signing flow before another command uses the device.
            let _ = client.call_raw(protos::Cancel::new());
        }
        result
    }

    fn sign_inner(&self, client: &mut Trezor, path: Vec<u32>) -> anyhow::Result<Signature> {
        let mut request = protos::EthereumSignTypedData::new();
        request.address_n = path;
        request.set_primary_type(self.primary_type.clone());
        request.set_metamask_v4_compat(true);
        let mut response = client.call_raw(request)?;
        loop {
            response = match response.message_type() {
                MessageType::MessageType_EthereumTypedDataStructRequest => {
                    let request: protos::EthereumTypedDataStructRequest =
                        response.into_message()?;
                    let mut ack = protos::EthereumTypedDataStructAck::new();
                    ack.members = self.struct_members(request.name())?;
                    client.call_raw(ack)?
                }
                MessageType::MessageType_EthereumTypedDataValueRequest => {
                    let request: protos::EthereumTypedDataValueRequest = response.into_message()?;
                    let mut ack = protos::EthereumTypedDataValueAck::new();
                    ack.set_value(self.value(&request.member_path)?);
                    client.call_raw(ack)?
                }
                MessageType::MessageType_ButtonRequest => {
                    let _: protos::ButtonRequest = response.into_message()?;
                    client.call_raw(protos::ButtonAck::new())?
                }
                MessageType::MessageType_PassphraseRequest => {
                    let _: protos::PassphraseRequest = response.into_message()?;
                    let mut ack = protos::PassphraseAck::new();
                    ack.set_on_device(true);
                    client.call_raw(ack)?
                }
                MessageType::MessageType_EthereumTypedDataSignature => {
                    let signature: protos::EthereumTypedDataSignature = response.into_message()?;
                    return Ok(Signature::from_raw(signature.signature())?);
                }
                MessageType::MessageType_Failure => {
                    let failure: protos::Failure = response.into_message()?;
                    return Err(trezor_client::Error::FailureResponse(failure).into());
                }
                other => bail!("unexpected Trezor signing response: {other:?}"),
            };
        }
    }

    fn struct_members(&self, name: &str) -> anyhow::Result<Vec<EthereumStructMember>> {
        let DynSolType::CustomStruct {
            prop_names, tuple, ..
        } = self.resolver.resolve(name)?
        else {
            bail!("Trezor requested a non-struct type: {name}");
        };
        prop_names
            .into_iter()
            .zip(tuple)
            .map(|(name, ty)| {
                let mut member = EthereumStructMember::new();
                member.set_name(name);
                member.type_ = Some(field_type(&ty)?).into();
                Ok(member)
            })
            .collect()
    }

    fn value(&self, path: &[u32]) -> anyhow::Result<Vec<u8>> {
        let (root, path) = path.split_first().context("empty Trezor member path")?;
        let mut value = match root {
            0 => &self.domain,
            1 => &self.message,
            _ => bail!("invalid Trezor member root: {root}"),
        };
        for index in path {
            let children = match value {
                DynSolValue::CustomStruct { tuple, .. } => tuple,
                DynSolValue::Array(items) | DynSolValue::FixedArray(items) => items,
                _ => bail!("Trezor member path traverses an atomic value"),
            };
            value = children
                .get(usize::try_from(*index)?)
                .context("invalid Trezor member index")?;
        }
        Ok(match value {
            DynSolValue::Uint(value, bits) => value.to_be_bytes::<32>()[32 - bits / 8..].to_vec(),
            DynSolValue::Int(value, bits) => value.to_be_bytes::<32>()[32 - bits / 8..].to_vec(),
            DynSolValue::FixedBytes(value, size) => value[..*size].to_vec(),
            DynSolValue::Bytes(value) => value.clone(),
            DynSolValue::String(value) => value.as_bytes().to_vec(),
            DynSolValue::Bool(value) => vec![u8::from(*value)],
            DynSolValue::Address(value) => value.to_vec(),
            DynSolValue::Array(items) | DynSolValue::FixedArray(items) => {
                u16::try_from(items.len())
                    .context("Trezor array exceeds 65535 elements")?
                    .to_be_bytes()
                    .to_vec()
            }
            _ => bail!("Trezor requested a non-atomic value"),
        })
    }
}

fn field_type(ty: &DynSolType) -> anyhow::Result<EthereumFieldType> {
    let mut field = EthereumFieldType::new();
    match ty {
        DynSolType::Uint(bits) | DynSolType::Int(bits) => {
            field.set_data_type(if matches!(ty, DynSolType::Uint(_)) {
                EthereumDataType::UINT
            } else {
                EthereumDataType::INT
            });
            field.set_size(u32::try_from(bits / 8)?);
        }
        DynSolType::FixedBytes(size) => {
            field.set_data_type(EthereumDataType::BYTES);
            field.set_size(u32::try_from(*size)?);
        }
        DynSolType::Bytes => field.set_data_type(EthereumDataType::BYTES),
        DynSolType::String => field.set_data_type(EthereumDataType::STRING),
        DynSolType::Bool => field.set_data_type(EthereumDataType::BOOL),
        DynSolType::Address => field.set_data_type(EthereumDataType::ADDRESS),
        DynSolType::Array(inner) | DynSolType::FixedArray(inner, _) => {
            ensure!(
                !matches!(**inner, DynSolType::Array(_) | DynSolType::FixedArray(..)),
                "Trezor does not support nested arrays"
            );
            field.set_data_type(EthereumDataType::ARRAY);
            if let DynSolType::FixedArray(_, size) = ty {
                field.set_size(u32::try_from(*size)?);
            }
            field.entry_type = Some(field_type(inner)?).into();
        }
        DynSolType::CustomStruct { name, tuple, .. } => {
            field.set_data_type(EthereumDataType::STRUCT);
            field.set_size(u32::try_from(tuple.len())?);
            field.set_struct_name(name.clone());
        }
        _ => bail!("unsupported Trezor field type: {ty}"),
    }
    Ok(field)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use super::*;
    use alloy::primitives::{Address, B256, U256};
    use serde_json::json;
    use trezor_client::{
        Model, TrezorMessage,
        client::trezor_with_transport,
        transport::{ProtoMessage, Transport, error::Error as TransportError},
    };

    #[derive(Default)]
    struct Exchange {
        requests: Vec<ProtoMessage>,
        responses: VecDeque<ProtoMessage>,
    }

    struct FakeTransport(Arc<Mutex<Exchange>>);

    impl Transport for FakeTransport {
        fn session_begin(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
        fn session_end(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
        fn write_message(&mut self, message: ProtoMessage) -> Result<(), TransportError> {
            self.0.lock().unwrap().requests.push(message);
            Ok(())
        }
        fn read_message(&mut self) -> Result<ProtoMessage, TransportError> {
            self.0
                .lock()
                .unwrap()
                .responses
                .pop_front()
                .ok_or(TransportError::DeviceReadTimeout)
        }
    }

    fn response<M: TrezorMessage>(message: M) -> ProtoMessage {
        ProtoMessage(M::MESSAGE_TYPE, message.write_to_bytes().unwrap())
    }

    fn client(responses: Vec<ProtoMessage>) -> (Trezor, Arc<Mutex<Exchange>>) {
        let exchange = Arc::new(Mutex::new(Exchange {
            requests: Vec::new(),
            responses: responses.into(),
        }));
        (
            trezor_with_transport(Model::Trezor, Box::new(FakeTransport(exchange.clone()))),
            exchange,
        )
    }

    fn payload(chain_id: U256) -> TypedData {
        serde_json::from_value(json!({
            "types": {
                "Item": [{"name": "amount", "type": "int256"}],
                "Message": [
                    {"name": "items", "type": "Item[]"},
                    {"name": "hash", "type": "bytes32"},
                    {"name": "enabled", "type": "bool"}
                ]
            },
            "primaryType": "Message",
            "domain": {"chainId": chain_id, "verifyingContract": Address::ZERO},
            "message": {"items": [{"amount": "-170141183460469231731687303715884105729"}],
                        "hash": B256::repeat_byte(42), "enabled": true}
        }))
        .unwrap()
    }

    #[test]
    fn streams_domain_and_full_width_values() {
        for chain_id in [U256::from(1337), U256::from(42161), U256::MAX] {
            let stream = TypedDataStream::new(&payload(chain_id)).unwrap();
            assert_eq!(stream.value(&[0, 0]).unwrap(), chain_id.to_be_bytes::<32>());
            assert_eq!(stream.value(&[0, 1]).unwrap(), Address::ZERO.to_vec());
            assert_eq!(stream.value(&[1, 0]).unwrap(), 1u16.to_be_bytes());
            let mut negative = [0xff; 32];
            negative[16..].copy_from_slice(&i128::MAX.to_be_bytes());
            assert_eq!(stream.value(&[1, 0, 0, 0]).unwrap(), negative);
            assert_eq!(stream.value(&[1, 1]).unwrap(), [42; 32]);
            assert_eq!(stream.value(&[1, 2]).unwrap(), [1]);
            let members = stream.struct_members("Message").unwrap();
            let array = members[0].type_.as_ref().unwrap();
            assert_eq!(array.data_type(), EthereumDataType::ARRAY);
            assert_eq!(array.entry_type.as_ref().unwrap().struct_name(), "Item");
        }
    }

    #[test]
    fn domain_definition_matches_only_populated_fields() {
        for (domain, names) in [
            (json!({}), vec![]),
            (
                json!({"name": "Test", "salt": B256::repeat_byte(7)}),
                vec!["name", "salt"],
            ),
        ] {
            let mut data = payload(U256::from(1337));
            data.domain = serde_json::from_value(domain).unwrap();
            data.resolver
                .ingest_string("EIP712Domain(uint256 chainId)")
                .unwrap();
            let stream = TypedDataStream::new(&data).unwrap();
            let members = stream.struct_members("EIP712Domain").unwrap();
            assert_eq!(
                members
                    .iter()
                    .map(|member| member.name())
                    .collect::<Vec<_>>(),
                names
            );
            if !members.is_empty() {
                assert_eq!(stream.value(&[0, 0]).unwrap(), b"Test");
                assert_eq!(stream.value(&[0, 1]).unwrap(), [7; 32]);
            }
        }
    }

    #[test]
    fn rejects_invalid_requests_and_conflicting_domains() {
        let mut data = payload(U256::from(1337));
        let stream = TypedDataStream::new(&data).unwrap();
        for path in [&[][..], &[2], &[1], &[1, 9], &[1, 2, 0]] {
            assert!(stream.value(path).is_err(), "{path:?}");
        }
        assert!(stream.struct_members("Unknown").is_err());
        assert!(
            field_type(&DynSolType::Array(Box::new(DynSolType::Array(Box::new(
                DynSolType::Bool
            )))))
            .is_err()
        );
        data.resolver
            .ingest_string("EIP712Domain(string name)")
            .unwrap();
        data.resolver
            .ingest_string("Message(EIP712Domain inner)")
            .unwrap();
        assert!(TypedDataStream::new(&data).is_err());
    }

    #[test]
    fn signing_exchanges_fields_and_device_confirmations() {
        let expected = Signature::new(U256::from(1), U256::from(2), true);
        let mut signature = protos::EthereumTypedDataSignature::new();
        signature.set_signature(expected.as_bytes().to_vec());
        signature.set_address(Address::ZERO.to_string());
        let mut structure = protos::EthereumTypedDataStructRequest::new();
        structure.set_name("EIP712Domain".into());
        let mut value = protos::EthereumTypedDataValueRequest::new();
        value.member_path = vec![0, 0];
        let (mut client, exchange) = client(vec![
            response(protos::ButtonRequest::new()),
            response(protos::PassphraseRequest::new()),
            response(structure),
            response(value),
            response(signature),
        ]);
        let stream = TypedDataStream::new(&payload(U256::from(1337))).unwrap();
        let path = vec![0x8000_002c, 0x8000_003c, 0x8000_0000, 0, 0];
        assert_eq!(stream.sign(&mut client, path.clone()).unwrap(), expected);
        let mut exchange = exchange.lock().unwrap();
        assert!(exchange.responses.is_empty());
        let mut requests = exchange.requests.drain(..);
        let start: protos::EthereumSignTypedData = requests.next().unwrap().into_message().unwrap();
        assert_eq!(start.address_n, path);
        assert_eq!(start.primary_type(), "Message");
        assert!(start.metamask_v4_compat());
        assert_eq!(
            requests.next().unwrap().message_type(),
            MessageType::MessageType_ButtonAck
        );
        let passphrase: protos::PassphraseAck = requests.next().unwrap().into_message().unwrap();
        assert!(passphrase.on_device());
        let members: protos::EthereumTypedDataStructAck =
            requests.next().unwrap().into_message().unwrap();
        assert_eq!(members.members[0].name(), "chainId");
        let value: protos::EthereumTypedDataValueAck =
            requests.next().unwrap().into_message().unwrap();
        assert_eq!(value.value(), U256::from(1337).to_be_bytes::<32>());
    }

    #[test]
    fn cancels_invalid_requests_and_malformed_signatures() {
        let mut invalid = protos::EthereumTypedDataValueRequest::new();
        invalid.member_path = vec![2];
        let mut malformed = protos::EthereumTypedDataSignature::new();
        malformed.set_signature(vec![0; 64]);
        malformed.set_address(Address::ZERO.to_string());
        for reply in [response(invalid), response(malformed)] {
            let (mut client, exchange) = client(vec![reply, response(protos::Failure::new())]);
            let stream = TypedDataStream::new(&payload(U256::from(1337))).unwrap();
            assert!(stream.sign(&mut client, vec![0]).is_err());
            let exchange = exchange.lock().unwrap();
            assert_eq!(
                exchange.requests.last().unwrap().message_type(),
                MessageType::MessageType_Cancel
            );
            assert!(exchange.responses.is_empty());
        }
    }
}
