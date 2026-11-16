//! Trezor signing and address discovery. Only address-to-path hints are persisted.

mod eip712;

use std::{env::home_dir, fmt, fs, io::Write, path::PathBuf};

use alloy::{
    dyn_abi::TypedData,
    primitives::{Address, B256, ChainId, Signature, U256, normalize_v},
    signers::Signer,
};
use anyhow::Context;
use async_trait::async_trait;
use bip32::{ChildNumber, DerivationPath, XPub};
use clap::Args;
use trezor_client::{
    client::{Trezor, handle_interaction},
    protos,
};

const PARENT_PATH: &str = "m/44'/60'/0'/0";

#[derive(Args, Debug, Clone)]
pub struct TrezorArgs {
    /// Use a specific Trezor derivation path (skips Ledger discovery).
    #[arg(long, value_parser = parse_path, conflicts_with = "trezor_index")]
    pub trezor_path: Option<DerivationPath>,
    /// Use Trezor address m/44'/60'/0'/0/INDEX (skips Ledger discovery).
    #[arg(long, value_parser = clap::value_parser!(u32).range(0..0x8000_0000))]
    pub trezor_index: Option<u32>,
    /// Number of Trezor addresses to derive locally when searching for authorized signers.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..=1_000_000))]
    pub trezor_scan_limit: u32,
}

impl Default for TrezorArgs {
    fn default() -> Self {
        Self {
            trezor_path: None,
            trezor_index: None,
            trezor_scan_limit: 10,
        }
    }
}

impl TrezorArgs {
    pub fn is_explicit(&self) -> bool {
        self.trezor_path.is_some() || self.trezor_index.is_some()
    }

    fn selected_path(&self) -> anyhow::Result<Option<DerivationPath>> {
        match (&self.trezor_path, self.trezor_index) {
            (Some(path), _) => Ok(Some(path.clone())),
            (None, Some(index)) => Ok(Some(path_for(index)?)),
            _ => Ok(None),
        }
    }
}

fn parse_path(value: &str) -> anyhow::Result<DerivationPath> {
    let path: DerivationPath = value.parse().context("invalid Trezor derivation path")?;
    anyhow::ensure!(
        !path.is_empty() && path.len() <= 10,
        "Trezor paths must have 1–10 components"
    );
    Ok(path)
}

fn path_for(index: u32) -> anyhow::Result<DerivationPath> {
    let mut path: DerivationPath = PARENT_PATH.parse()?;
    path.push(ChildNumber::new(index, false)?);
    Ok(path)
}

fn address_for(xpub: &XPub, index: u32) -> anyhow::Result<Address> {
    let child = xpub.derive_child(ChildNumber::new(index, false)?)?;
    let key = child.public_key().to_sec1_point(false);
    Ok(Address::from_raw_public_key(&key.as_bytes()[1..]))
}

trait AddressSource {
    fn address(&mut self, path: &DerivationPath) -> anyhow::Result<Address>;
    fn public_key(&mut self) -> anyhow::Result<XPub>;
}

impl AddressSource for Trezor {
    fn address(&mut self, path: &DerivationPath) -> anyhow::Result<Address> {
        self.ethereum_get_address(path.iter().map(u32::from).collect())?
            .parse()
            .context("invalid Ethereum address returned by Trezor")
    }

    fn public_key(&mut self) -> anyhow::Result<XPub> {
        let mut request = protos::EthereumGetPublicKey::new();
        request.address_n = PARENT_PATH
            .parse::<DerivationPath>()?
            .iter()
            .map(u32::from)
            .collect();
        request.set_show_display(false);
        let xpub = handle_interaction(self.call(
            request,
            Box::new(|_, response: protos::EthereumPublicKey| Ok(response.xpub().to_owned())),
        )?)?;
        xpub.parse()
            .context("invalid extended public key returned by Trezor")
    }
}

/// Each address has its own atomically replaced hint, so concurrent commands do
/// not overwrite hints for other addresses. Hints are always verified on-device.
struct PathCache(Option<PathBuf>);

impl PathCache {
    fn load(&self, address: Address) -> Option<DerivationPath> {
        let file = self.0.as_ref()?.join(format!("{address:x}"));
        parse_path(fs::read_to_string(file).ok()?.trim()).ok()
    }

    fn save(&self, address: Address, path: &DerivationPath) -> anyhow::Result<()> {
        if let Some(directory) = &self.0 {
            fs::create_dir_all(directory)?;
            let mut file = tempfile::NamedTempFile::new_in(directory)?;
            writeln!(file, "{path}")?;
            file.persist(directory.join(format!("{address:x}")))?;
        }
        Ok(())
    }
}

fn discover(
    device: &mut impl AddressSource,
    args: &TrezorArgs,
    filter_by: Option<&[Address]>,
    first_only: bool,
    cache: &PathCache,
) -> anyhow::Result<Vec<(DerivationPath, Address)>> {
    if filter_by.is_some_and(|addresses| addresses.is_empty()) {
        return Ok(Vec::new());
    }
    let selected = args.selected_path()?;
    if selected.is_some() || filter_by.is_none() {
        let path = selected.unwrap_or(path_for(0)?);
        let address = device.address(&path)?;
        anyhow::ensure!(
            filter_by.is_none_or(|addresses| addresses.contains(&address)),
            "Trezor address {address} at {path} is not an authorized signer"
        );
        return Ok(vec![(path, address)]);
    }

    let mut remaining = filter_by.unwrap().to_vec();
    remaining.sort_unstable();
    remaining.dedup();
    let mut found = Vec::new();
    for address in remaining.clone() {
        if let Some(path) = cache.load(address)
            && device.address(&path)? == address
        {
            found.push((path, address));
            remaining.retain(|candidate| *candidate != address);
            if first_only || remaining.is_empty() {
                return Ok(found);
            }
        }
    }

    let xpub = device.public_key().context(
        "could not retrieve Trezor public key for discovery; use --trezor-path or --trezor-index to select an address directly",
    )?;
    for index in 0..args.trezor_scan_limit {
        let address = address_for(&xpub, index)?;
        if remaining.contains(&address) {
            let path = path_for(index)?;
            anyhow::ensure!(
                device.address(&path)? == address,
                "Trezor address does not match the derived public key at {path}"
            );
            found.push((path, address));
            remaining.retain(|candidate| *candidate != address);
            if first_only || remaining.is_empty() {
                break;
            }
        }
    }
    Ok(found)
}

pub fn find_signers(
    args: &TrezorArgs,
    filter_by: Option<&[Address]>,
    first_only: bool,
) -> anyhow::Result<Vec<TrezorTypedDataSigner>> {
    if filter_by.is_some_and(|addresses| addresses.is_empty()) {
        return Ok(Vec::new());
    }
    let mut client = match trezor_client::unique(false) {
        Ok(client) => client,
        Err(trezor_client::Error::NoDeviceFound) if !args.is_explicit() => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    client.init_device(None)?;
    let cache = PathCache(home_dir().map(|home| home.join(".cache/hypecli/trezor-paths")));
    let found = discover(&mut client, args, filter_by, first_only, &cache)?;
    let session_id = client
        .features()
        .ok_or_else(|| anyhow::anyhow!("could not retrieve Trezor features"))?
        .session_id()
        .to_vec();
    Ok(found
        .into_iter()
        .map(|(path, address)| {
            if let Err(error) = cache.save(address, &path) {
                eprintln!("Could not save Trezor path for {address}: {error}");
            }
            TrezorTypedDataSigner {
                path,
                session_id: session_id.clone(),
                address,
                chain_id: Some(1),
            }
        })
        .collect())
}

pub struct TrezorTypedDataSigner {
    path: DerivationPath,
    session_id: Vec<u8>,
    address: Address,
    chain_id: Option<ChainId>,
}

impl fmt::Debug for TrezorTypedDataSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrezorTypedDataSigner")
            .field("path", &self.path)
            .field("address", &self.address)
            .finish()
    }
}

#[async_trait]
impl Signer for TrezorTypedDataSigner {
    async fn sign_message(&self, message: &[u8]) -> alloy::signers::Result<Signature> {
        let mut client = self.client().map_err(alloy::signers::Error::other)?;
        let signature = client
            .ethereum_sign_message(message.to_vec(), self.path.iter().map(u32::from).collect())
            .map_err(alloy::signers::Error::other)?;
        signature_from_trezor(signature)
    }

    async fn sign_hash(&self, _hash: &B256) -> alloy::signers::Result<Signature> {
        Err(alloy::signers::Error::UnsupportedOperation(
            alloy::signers::UnsupportedSignerOperation::SignHash,
        ))
    }

    async fn sign_dynamic_typed_data(&self, data: &TypedData) -> alloy::signers::Result<Signature> {
        let stream = eip712::TypedDataStream::new(data).map_err(alloy::signers::Error::other)?;
        let mut client = self.client().map_err(alloy::signers::Error::other)?;
        let signature = stream
            .sign(&mut client, self.path.iter().map(u32::from).collect())
            .map_err(alloy::signers::Error::other)?;
        verify_typed_data_signature(signature, data, self.address)
    }

    fn address(&self) -> Address {
        self.address
    }
    fn chain_id(&self) -> Option<ChainId> {
        self.chain_id
    }
    fn set_chain_id(&mut self, chain_id: Option<ChainId>) {
        self.chain_id = chain_id;
    }
}

fn verify_typed_data_signature(
    signature: Signature,
    data: &TypedData,
    expected: Address,
) -> alloy::signers::Result<Signature> {
    let hash = data.eip712_signing_hash()?;
    let recovered = signature.recover_address_from_prehash(&hash)?;
    if recovered != expected {
        return Err(alloy::signers::Error::other(format!(
            "Trezor signature does not match requested typed data: expected {expected}, recovered {recovered}"
        )));
    }
    Ok(signature)
}

impl TrezorTypedDataSigner {
    fn client(&self) -> anyhow::Result<Trezor> {
        let mut client = trezor_client::unique(false)?;
        client.init_device(Some(self.session_id.clone()))?;
        anyhow::ensure!(
            client.address(&self.path)? == self.address,
            "connected Trezor wallet does not match signer {} at {}",
            self.address,
            self.path
        );
        Ok(client)
    }
}

fn signature_from_trezor(
    signature: trezor_client::client::Signature,
) -> alloy::signers::Result<Signature> {
    let v = normalize_v(signature.v).ok_or_else(|| {
        alloy::signers::Error::other(anyhow::anyhow!("invalid Trezor signature parity"))
    })?;
    Ok(Signature::new(
        U256::from_be_bytes(signature.r),
        U256::from_be_bytes(signature.s),
        v,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bip32::XPrv;
    use clap::{CommandFactory, Parser};
    use hypersdk::hypercore::PrivateKeySigner;

    #[tokio::test]
    async fn rejects_a_signature_for_different_typed_data() {
        let signer = PrivateKeySigner::random();
        let mut data: TypedData = serde_json::from_value(serde_json::json!({
            "types": {"Message": [{"name": "value", "type": "bool"}]},
            "primaryType": "Message",
            "domain": {"chainId": 1337},
            "message": {"value": true}
        }))
        .unwrap();
        let signature = signer.sign_dynamic_typed_data(&data).await.unwrap();
        verify_typed_data_signature(signature, &data, signer.address()).unwrap();
        data.domain.chain_id = Some(U256::ZERO);
        assert!(verify_typed_data_signature(signature, &data, signer.address()).is_err());
    }

    #[derive(Default)]
    struct FakeDevice {
        address_requests: Vec<DerivationPath>,
        public_key_requests: usize,
        cancel: bool,
        wrong_xpub: bool,
    }

    fn expected_address(path: &DerivationPath) -> Address {
        let key = XPrv::derive_from_path(&[7; 32], path).unwrap();
        PrivateKeySigner::from_bytes(&B256::from_slice(&key.to_bytes()))
            .unwrap()
            .address()
    }

    impl AddressSource for FakeDevice {
        fn address(&mut self, path: &DerivationPath) -> anyhow::Result<Address> {
            self.address_requests.push(path.clone());
            anyhow::ensure!(!self.cancel, "user cancelled");
            Ok(expected_address(path))
        }

        fn public_key(&mut self) -> anyhow::Result<XPub> {
            self.public_key_requests += 1;
            anyhow::ensure!(!self.cancel, "user cancelled");
            let seed = if self.wrong_xpub { [8; 32] } else { [7; 32] };
            Ok(XPrv::derive_from_path(&seed, &PARENT_PATH.parse()?)?.public_key())
        }
    }

    #[test]
    fn public_derivation_matches_private_derivation() {
        let xpub = FakeDevice::default().public_key().unwrap();
        for index in [0, 1, 9, 100, 0x7fff_ffff] {
            assert_eq!(
                address_for(&xpub, index).unwrap(),
                expected_address(&path_for(index).unwrap())
            );
        }
        assert!(address_for(&xpub, 0x8000_0000).is_err());
    }

    #[test]
    fn discovery_requests_one_xpub_and_only_verifies_matching_addresses() {
        let mut device = FakeDevice::default();
        let paths = [path_for(2).unwrap(), path_for(9).unwrap()];
        let addresses = paths.each_ref().map(expected_address);
        let found = discover(
            &mut device,
            &TrezorArgs::default(),
            Some(&addresses),
            false,
            &PathCache(None),
        )
        .unwrap();
        assert_eq!(found, paths.into_iter().zip(addresses).collect::<Vec<_>>());
        assert_eq!(device.public_key_requests, 1);
        assert_eq!(device.address_requests.len(), 2);
    }

    #[test]
    fn respects_scan_limit_and_first_match() {
        let addresses = [
            expected_address(&path_for(1).unwrap()),
            expected_address(&path_for(10).unwrap()),
        ];
        let mut device = FakeDevice::default();
        let args = TrezorArgs::default();
        let found = discover(
            &mut device,
            &args,
            Some(&addresses[1..]),
            false,
            &PathCache(None),
        )
        .unwrap();
        assert!(found.is_empty());
        assert!(device.address_requests.is_empty());
        let args = TrezorArgs {
            trezor_scan_limit: 11,
            ..args
        };
        let found = discover(
            &mut device,
            &args,
            Some(&addresses[1..]),
            false,
            &PathCache(None),
        )
        .unwrap();
        assert_eq!(found[0].1, addresses[1]);
        let found = discover(&mut device, &args, Some(&addresses), true, &PathCache(None)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, addresses[0]);
    }

    #[test]
    fn explicit_paths_and_unfiltered_default_skip_xpub() {
        let mut device = FakeDevice::default();
        let args = TrezorArgs {
            trezor_index: Some(100),
            ..TrezorArgs::default()
        };
        let found = discover(&mut device, &args, None, true, &PathCache(None)).unwrap();
        assert_eq!(found[0].0, path_for(100).unwrap());
        let path = parse_path("m/44'/60'/5'/0/0").unwrap();
        let args = TrezorArgs {
            trezor_path: Some(path.clone()),
            ..TrezorArgs::default()
        };
        let found = discover(
            &mut device,
            &args,
            Some(&[expected_address(&path)]),
            true,
            &PathCache(None),
        )
        .unwrap();
        assert_eq!(found[0].0, path);
        assert!(
            discover(
                &mut device,
                &args,
                Some(&[Address::ZERO]),
                true,
                &PathCache(None)
            )
            .is_err()
        );
        let found = discover(
            &mut device,
            &TrezorArgs::default(),
            None,
            true,
            &PathCache(None),
        )
        .unwrap();
        assert_eq!(found[0].0, path_for(0).unwrap());
        assert_eq!(device.public_key_requests, 0);
    }

    #[test]
    fn cached_path_beyond_scan_limit_is_verified_without_xpub() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PathCache(Some(directory.path().to_owned()));
        let path = path_for(100).unwrap();
        let address = expected_address(&path);
        cache.save(address, &path).unwrap();
        let mut device = FakeDevice::default();
        let found = discover(
            &mut device,
            &TrezorArgs::default(),
            Some(&[address]),
            false,
            &cache,
        )
        .unwrap();
        assert_eq!(found, vec![(path.clone(), address)]);
        assert_eq!(device.address_requests, vec![path]);
        assert_eq!(device.public_key_requests, 0);
    }

    #[test]
    fn stale_cache_falls_back_to_discovery_and_wrong_wallet_is_not_accepted() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PathCache(Some(directory.path().to_owned()));
        let path = path_for(3).unwrap();
        let address = expected_address(&path);
        cache.save(address, &path_for(1).unwrap()).unwrap();
        cache.save(Address::ZERO, &path_for(0).unwrap()).unwrap();
        let mut device = FakeDevice::default();
        let found = discover(
            &mut device,
            &TrezorArgs::default(),
            Some(&[address, Address::ZERO]),
            false,
            &cache,
        )
        .unwrap();
        assert_eq!(found, vec![(path.clone(), address)]);
        assert_eq!(device.public_key_requests, 1);
        cache.save(address, &path).unwrap();
        assert_eq!(cache.load(address), Some(path));
    }

    #[test]
    fn malformed_cache_is_ignored() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PathCache(Some(directory.path().to_owned()));
        let address = expected_address(&path_for(0).unwrap());
        fs::write(
            directory.path().join(format!("{address:x}")),
            "not a derivation path",
        )
        .unwrap();
        let mut device = FakeDevice::default();
        let found = discover(
            &mut device,
            &TrezorArgs::default(),
            Some(&[address]),
            false,
            &cache,
        )
        .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(device.public_key_requests, 1);
    }

    #[test]
    fn cancellation_stops_discovery_without_retrying() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PathCache(Some(directory.path().to_owned()));
        let path = path_for(0).unwrap();
        let address = expected_address(&path);
        cache.save(address, &path).unwrap();
        let mut device = FakeDevice {
            cancel: true,
            ..FakeDevice::default()
        };
        assert!(
            discover(
                &mut device,
                &TrezorArgs::default(),
                Some(&[address]),
                false,
                &cache
            )
            .is_err()
        );
        assert_eq!(device.address_requests.len(), 1);
        assert_eq!(device.public_key_requests, 0);
        assert!(
            discover(
                &mut device,
                &TrezorArgs::default(),
                Some(&[address]),
                false,
                &PathCache(None)
            )
            .is_err()
        );
        assert_eq!(device.public_key_requests, 1);
        assert_eq!(device.address_requests.len(), 1);
    }

    #[test]
    fn mismatched_xpub_is_rejected_and_empty_filter_does_not_contact_device() {
        let mut device = FakeDevice {
            wrong_xpub: true,
            ..FakeDevice::default()
        };
        let address = address_for(&device.public_key().unwrap(), 0).unwrap();
        assert!(
            discover(
                &mut device,
                &TrezorArgs::default(),
                Some(&[address]),
                false,
                &PathCache(None)
            )
            .is_err()
        );
        let mut device = FakeDevice::default();
        assert!(
            discover(
                &mut device,
                &TrezorArgs::default(),
                Some(&[]),
                false,
                &PathCache(None)
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(device.public_key_requests, 0);
        assert!(device.address_requests.is_empty());
    }

    #[test]
    fn cli_validates_paths_indices_limits_and_conflicting_sources() {
        crate::Cli::command().debug_assert();
        let parse = |args: &[&str]| {
            crate::Cli::try_parse_from(
                ["hypecli", "account", "test-signer"]
                    .into_iter()
                    .chain(args.iter().copied()),
            )
        };
        assert!(parse(&["--trezor-index", "100"]).is_ok());
        assert!(parse(&["--trezor-path", "m/44'/60'/5'/0/0"]).is_ok());
        for args in [
            vec!["--trezor-index", "2147483648"],
            vec!["--trezor-index", "-1"],
            vec!["--trezor-path", "m"],
            vec!["--trezor-path", "m/bad"],
            vec!["--trezor-path", "m/0", "--trezor-index", "0"],
            vec!["--trezor-scan-limit", "0"],
            vec!["--trezor-scan-limit", "1000001"],
        ] {
            assert!(parse(&args).is_err(), "accepted {args:?}");
        }

        #[derive(Parser)]
        struct SignerCli {
            #[command(flatten)]
            args: crate::SignerArgs,
        }
        for source in ["--private-key", "--keystore"] {
            let error = SignerCli::try_parse_from(["test", "--trezor-index", "0", source, "value"])
                .err()
                .unwrap();
            assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
        }
    }
}
