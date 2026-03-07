//! Utility functions for multi-sig operations and gossip networking.
//!
//! This module provides helper functions for:
//! - Creating gossip network topics from multi-sig addresses
//! - Finding and loading signers (private keys, keystores, Ledger)
//! - Starting gossip nodes for peer-to-peer communication
//! - Signing multi-sig actions
//! - Parsing unified asset name formats
//! - Keystore directory management
//! - Fuzzy matching for better error messages
//! - Common query arguments and formatting

use std::path::PathBuf;
use std::{env::home_dir, str::FromStr};

use alloy::signers::{self, Signer, ledger::LedgerSigner};
use anyhow::Context;
use clap::ValueEnum;
use hypersdk::{Address, hypercore::PrivateKeySigner};
use strsim::levenshtein;
use iroh::{
    Endpoint, SecretKey,
    discovery::{dns::DnsDiscovery, mdns::MdnsDiscovery},
};
use iroh_tickets::endpoint::EndpointTicket;

use hypersdk::hypercore::{HttpClient, PerpMarket, SpotMarket};

use crate::SignerArgs;

/// Find similar symbols to a given input string.
///
/// Returns the top 3 closest matches from the candidates, sorted by
/// Levenshtein distance. Only returns matches within a reasonable
/// distance threshold (max 3 edits for typical ticker symbols).
fn find_similar_symbols(candidates: &[&str], input: &str, max_results: usize) -> Vec<String> {
    let mut scored: Vec<(usize, &str)> = candidates
        .iter()
        .copied()
        .filter(|c| !c.eq_ignore_ascii_case(input))
        .map(|c| (levenshtein(c, input), c))
        .filter(|(dist, _)| *dist <= 3) // Only reasonable matches
        .collect();

    scored.sort_by_key(|&(dist, _)| dist);
    scored.truncate(max_results);

    scored.into_iter().map(|(_, sym)| sym.to_string()).collect()
}

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable formatted output
    #[default]
    Pretty,
    /// Tab-aligned table output
    Table,
    /// JSON output for programmatic consumption
    Json,
}

/// Common query arguments for order/fills queries.
#[derive(clap::Args)]
pub struct QueryArgs {
    /// Filter by status (open, filled, canceled, all)
    #[arg(long, default_value = "all")]
    pub status: String,
    
    /// Output format
    #[arg(long, default_value = "pretty")]
    pub format: OutputFormat,
    
    /// Limit number of results
    #[arg(long, default_value = "100")]
    pub limit: usize,
}

/// Get the default keystore directory path (~/.foundry/keystores).
pub fn keystore_dir() -> anyhow::Result<PathBuf> {
    let home = home_dir().ok_or_else(|| anyhow::anyhow!("Unable to locate home directory"))?;
    Ok(home.join(".foundry").join("keystores"))
}

/// Generates a random secret key for the gossip node.
pub fn make_key(_signer: &impl Signer) -> SecretKey {
    // let public_address = signer.address();
    // let mut address_bytes = [0u8; 32];
    // address_bytes[0..20].copy_from_slice(&public_address[..]);
    // SecretKey::from_bytes(&address_bytes)
    SecretKey::generate(&mut rand_09::rng())
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

/// Parsed asset specification.
///
/// Represents different asset types that can be specified using the unified format.
#[derive(Debug, Clone)]
pub enum AssetSpec<'a> {
    /// Perpetual on Hyperliquid DEX (e.g., "BTC")
    Perp(&'a str),
    /// Spot market (e.g., "PURR/USDC")
    Spot(&'a str, &'a str),
    /// Perpetual on HIP3 DEX (e.g., "xyz:BTC")
    Hip3Perp(&'a str, &'a str),
}

/// Parse an asset name string into an AssetSpec.
///
/// # Formats
///
/// - `"BTC"` → `Perp("BTC")` - Perpetual on Hyperliquid DEX
/// - `"PURR/USDC"` → `Spot("PURR", "USDC")` - Spot market
/// - `"xyz:BTC"` → `Hip3Perp("xyz", "BTC")` - Perpetual on HIP3 DEX
///
/// # Examples
///
/// ```
/// let spec = parse_asset_spec("BTC").unwrap();
/// // Returns AssetSpec::Perp("BTC")
///
/// let spec = parse_asset_spec("PURR/USDC").unwrap();
/// // Returns AssetSpec::Spot("PURR", "USDC")
///
/// let spec = parse_asset_spec("xyz:BTC").unwrap();
/// // Returns AssetSpec::Hip3Perp("xyz", "BTC")
/// ```
pub fn parse_asset_spec(asset: &str) -> anyhow::Result<AssetSpec<'_>> {
    if let Some((base, quote)) = asset.split_once('/') {
        // Spot market: BASE/QUOTE
        Ok(AssetSpec::Spot(base, quote))
    } else if let Some((dex, symbol)) = asset.split_once(':') {
        // HIP3 DEX: dex:SYMBOL
        Ok(AssetSpec::Hip3Perp(dex, symbol))
    } else {
        // Default: Hyperliquid perp
        Ok(AssetSpec::Perp(asset))
    }
}

/// Resolve an asset name to its index.
///
/// This queries the appropriate market data based on the asset format
/// and returns the asset index for use in API calls.
///
/// # Arguments
///
/// * `client` - HTTP client for querying market data
/// * `asset` - Asset name in unified format (e.g., "BTC", "PURR/USDC", "xyz:BTC")
///
/// # Returns
///
/// The asset index for use in API calls.
pub async fn resolve_asset(client: &HttpClient, asset: &str) -> anyhow::Result<usize> {
    let spec = parse_asset_spec(asset)?;

    match spec {
        AssetSpec::Perp(symbol) => {
            let perps = client.perps().await?;
            find_perp_index(&perps, symbol)
        }
        AssetSpec::Spot(base, quote) => {
            let spots = client.spot().await?;
            find_spot_index(&spots, base, quote)
        }
        AssetSpec::Hip3Perp(dex_name, symbol) => {
            // First get the DEX
            let dexs = client.perp_dexs().await?;
            let dex = dexs
                .iter()
                .find(|d| d.name().eq_ignore_ascii_case(dex_name))
                .ok_or_else(|| anyhow::anyhow!("HIP3 DEX '{}' not found", dex_name))?;

            // Then get perps from that DEX
            let perps = client.perps_from(dex.clone()).await?;
            find_perp_index_with_dex(&perps, symbol, Some(dex_name))
        }
    }
}

/// Find a perpetual market index by symbol with fuzzy matching suggestions.
fn find_perp_index(perps: &[PerpMarket], symbol: &str) -> anyhow::Result<usize> {
    // First try exact match
    if let Some(index) = perps
        .iter()
        .position(|p| p.name.eq_ignore_ascii_case(symbol))
    {
        return Ok(index);
    }
    
    // Extract all candidate symbols for fuzzy matching
    let candidates: Vec<&str> = perps
        .iter()
        .map(|p| {
            // For HIP3 format "dex:symbol", use just the symbol part
            if let Some((_dex, sym)) = p.name.split_once(':') {
                sym
            } else {
                &p.name
            }
        })
        .collect();
    
    // Find similar symbols
    let similar = find_similar_symbols(&candidates, symbol, 3);
    
    if similar.is_empty() {
        Err(anyhow::anyhow!(
            "Perpetual market '{}' not found. Use 'hypecli perps' to list available markets.",
            symbol
        ))
    } else {
        let suggestions = similar.join(", ");
        Err(anyhow::anyhow!(
            "Perpetual market '{}' not found. Did you mean: {}?",
            symbol,
            suggestions
        ))
    }
}

/// Find a perpetual market index for HIP3 perps with fuzzy matching.
fn find_perp_index_with_dex(
    perps: &[PerpMarket],
    symbol: &str,
    _dex_name: Option<&str>,
) -> anyhow::Result<usize> {
    // First try exact match
    if let Some(index) = perps.iter().position(|p| perp_name_matches(&p.name, symbol)) {
        return Ok(index);
    }
    
    // Extract all candidate symbols for fuzzy matching
    let candidates: Vec<&str> = perps
        .iter()
        .map(|p| {
            if let Some((_dex, sym)) = p.name.split_once(':') {
                sym
            } else {
                &p.name
            }
        })
        .collect();
    
    // Find similar symbols
    let similar = find_similar_symbols(&candidates, symbol, 3);
    
    if similar.is_empty() {
        Err(anyhow::anyhow!(
            "Perpetual market '{}' not found. Use 'hypecli perps' to list available markets.",
            symbol
        ))
    } else {
        let suggestions = similar.join(", ");
        Err(anyhow::anyhow!(
            "Perpetual market '{}' not found. Did you mean: {}?",
            symbol,
            suggestions
        ))
    }
}

/// Check if a perp market name matches the given symbol.
///
/// Matches either:
/// - Exact match (case-insensitive): "BTC" matches "BTC"
/// - HIP3 format with prefix: "BTC" matches "xyz:BTC"
fn perp_name_matches(name: &str, symbol: &str) -> bool {
    if name.eq_ignore_ascii_case(symbol) {
        return true;
    }
    // Check for "dex:SYMBOL" format
    if let Some((_dex, market_symbol)) = name.split_once(':') {
        return market_symbol.eq_ignore_ascii_case(symbol);
    }
    false
}

/// Find a spot market index by base and quote symbols with fuzzy matching.
fn find_spot_index(spots: &[SpotMarket], base: &str, quote: &str) -> anyhow::Result<usize> {
    // First try exact match
    if let Some(index) = spots.iter().position(|s| {
        s.base().name.eq_ignore_ascii_case(base) && s.quote().name.eq_ignore_ascii_case(quote)
    }) {
        return Ok(index);
    }
    
    // Try fuzzy match on base
    let base_candidates: Vec<&str> = spots.iter().map(|s| s.base().name.as_str()).collect();
    let similar_base = find_similar_symbols(&base_candidates, base, 3);
    
    if !similar_base.is_empty() {
        let suggestions = similar_base.join(", ");
        return Err(anyhow::anyhow!(
            "Spot market base '{}' not found. Did you mean: {}?",
            base,
            suggestions
        ));
    }
    
    // Try fuzzy match on quote
    let quote_candidates: Vec<&str> = spots.iter().map(|s| s.quote().name.as_str()).collect();
    let similar_quote = find_similar_symbols(&quote_candidates, quote, 3);
    
    if !similar_quote.is_empty() {
        let suggestions = similar_quote.join(", ");
        return Err(anyhow::anyhow!(
            "Spot market quote '{}' not found. Did you mean: {}?",
            quote,
            suggestions
        ));
    }
    
    Err(anyhow::anyhow!(
        "Spot market '{}/{}' not found. Use 'hypecli spot' to list available markets.",
        base,
        quote
    ))
}

/// Resolved asset information for subscriptions.
///
/// Contains the coin name to use for WebSocket subscriptions.
#[derive(Debug, Clone)]
pub struct ResolvedAsset {
    /// The coin name to use for subscriptions (e.g., "BTC", "PURR", "@123")
    pub coin: String,
}

/// Resolve an asset name to subscription parameters.
///
/// This resolves the unified asset format to the coin name and DEX
/// needed for WebSocket subscriptions.
///
/// # Arguments
///
/// * `client` - HTTP client for querying market data
/// * `asset` - Asset name in unified format (e.g., "BTC", "PURR/USDC", "xyz:BTC")
///
/// # Returns
///
/// A `ResolvedAsset` containing the coin name and optional DEX for subscriptions.
pub async fn resolve_asset_for_subscription(
    client: &HttpClient,
    asset: &str,
) -> anyhow::Result<ResolvedAsset> {
    let spec = parse_asset_spec(asset)?;

    match spec {
        AssetSpec::Perp(symbol) => {
            // Verify the perp exists
            let perps = client.perps().await?;
            let perp = perps
                .iter()
                .find(|p| perp_name_matches(&p.name, symbol))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Perpetual market '{}' not found. Use 'hypecli perps' to list available markets.",
                        symbol
                    )
                })?;
            Ok(ResolvedAsset {
                coin: perp.name.clone(),
            })
        }
        AssetSpec::Spot(base, quote) => {
            // Verify the spot market exists and get its index for the @-prefixed coin name
            let spots = client.spot().await?;
            let spot = spots
                .iter()
                .find(|s| {
                    s.base().name.eq_ignore_ascii_case(base)
                        && s.quote().name.eq_ignore_ascii_case(quote)
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Spot market '{}/{}' not found. Use 'hypecli spot' to list available markets.",
                        base,
                        quote
                    )
                })?;
            // Spot subscriptions use @index format
            Ok(ResolvedAsset {
                coin: format!("@{}", spot.index - 10_000),
            })
        }
        AssetSpec::Hip3Perp(dex_name, symbol) => {
            // First get the DEX
            let dexs = client.perp_dexs().await?;
            let dex = dexs
                .iter()
                .find(|d| d.name().eq_ignore_ascii_case(dex_name))
                .ok_or_else(|| anyhow::anyhow!("HIP3 DEX '{}' not found", dex_name))?;

            // Then verify the perp exists on that DEX
            let perps = client.perps_from(dex.clone()).await?;
            let perp = perps
                .iter()
                .find(|p| perp_name_matches(&p.name, symbol))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Perpetual market '{}' on {} DEX not found.",
                        symbol,
                        dex_name
                    )
                })?;

            Ok(ResolvedAsset {
                coin: perp.name.clone(),
            })
        }
    }
}
