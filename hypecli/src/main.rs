mod balances;
mod markets;
mod morpho;
mod multisig;
mod orders;
mod send;
mod subscribe;
mod to_multisig;
mod utils;

use balances::SpotBalancesCmd;
use clap::{Args, Parser};
use hypersdk::hypercore::Chain;
use markets::{PerpsCmd, SpotCmd};
use morpho::{MorphoApyCmd, MorphoPositionCmd, MorphoVaultApyCmd};
use multisig::MultiSigCmd;
use orders::OrderCmd;
use send::SendCmd;
use subscribe::SubscribeCmd;
use to_multisig::ToMultiSigCmd;

/// Main CLI structure for hypecli - A command-line interface for Hyperliquid.
#[derive(Parser)]
#[command(author, version)]
#[allow(clippy::large_enum_variant)]
struct Cli {
    /// Show detailed help for AI agents
    #[arg(long)]
    agent_help: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// List perpetual markets
    Perps(PerpsCmd),
    /// List spot markets
    Spot(SpotCmd),
    /// Gather spot balances for a user.
    SpotBalances(SpotBalancesCmd),
    /// Query an addresses' morpho balance
    MorphoPosition(MorphoPositionCmd),
    /// Query APY for a Morpho market
    MorphoApy(MorphoApyCmd),
    /// Query APY for a MetaMorpho vault
    MorphoVaultApy(MorphoVaultApyCmd),
    /// Multi-sig commands
    #[command(subcommand)]
    Multisig(MultiSigCmd),
    /// Convert a regular user to a multi-sig user
    ToMultisig(ToMultiSigCmd),
    /// Order management (place and cancel orders)
    #[command(subcommand)]
    Order(OrderCmd),
    /// Subscribe to real-time WebSocket data feeds
    #[command(subcommand)]
    Subscribe(SubscribeCmd),
    /// Send assets between accounts, DEXes, or subaccounts
    Send(SendCmd),
}

impl Command {
    async fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Perps(cmd) => cmd.run().await,
            Self::Spot(cmd) => cmd.run().await,
            Self::SpotBalances(cmd) => cmd.run().await,
            Self::MorphoPosition(cmd) => cmd.run().await,
            Self::MorphoApy(cmd) => cmd.run().await,
            Self::MorphoVaultApy(cmd) => cmd.run().await,
            Self::Multisig(cmd) => cmd.run().await,
            Self::ToMultisig(cmd) => cmd.run().await,
            Self::Order(cmd) => cmd.run().await,
            Self::Subscribe(cmd) => cmd.run().await,
            Self::Send(cmd) => cmd.run().await,
        }
    }
}

/// Common arguments for multi-signature commands.
///
/// These arguments are shared across all multi-sig operations to specify
/// the signer credentials and target multi-sig wallet.
#[derive(Args)]
pub struct SignerArgs {
    /// Private key for signing (hex format).
    #[arg(long)]
    pub private_key: Option<String>,
    /// Foundry keystore.
    #[arg(long)]
    pub keystore: Option<String>,
    /// Keystore password. Otherwise it'll be prompted.
    #[arg(long)]
    pub password: Option<String>,
    /// Target chain for the operation.
    #[arg(long, default_value = "mainnet")]
    pub chain: Chain,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.agent_help {
        print_agent_help();
        return Ok(());
    }

    match cli.command {
        Some(cmd) => cmd.run().await,
        None => {
            // No command provided, show help
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}

fn print_agent_help() {
    print!(
        r#"HYPECLI - AI Agent Guide
========================

This CLI provides commands for interacting with the Hyperliquid decentralized exchange.
Below is a comprehensive guide for AI agents on how to use each command.

CHAIN SELECTION
---------------
Most commands require a `--chain` argument. Valid values are:
  - mainnet  Production Hyperliquid network
  - testnet  Test network for development

AUTHENTICATION
--------------
Commands that modify state (orders, transfers, etc.) require authentication via SignerArgs:
  --private-key <HEX>   Direct private key (with or without 0x prefix)
  --keystore <NAME>     Foundry keystore name (located in ~/.foundry/keystores/)
  --password <PASS>     Keystore password (prompted if not provided)

Note: Ledger hardware wallets are supported for multi-sig operations but NOT for
order placement/cancellation (which require synchronous signing).

ASSET NAME FORMATS
------------------
Order commands use human-readable asset names with automatic index resolution:

  Format        Example         Description
  ──────────────────────────────────────────────────────────
  SYMBOL        BTC, ETH        Perpetual on Hyperliquid DEX
  BASE/QUOTE    PURR/USDC       Spot market
  dex:SYMBOL    xyz:BTC         Perpetual on HIP3 DEX

Use `hypecli perps` or `hypecli spot` to list available markets.

QUERY COMMANDS (No Authentication Required)
-------------------------------------------

List Perpetual Markets:
  hypecli perps

List Spot Markets:
  hypecli spot

Query Spot Balances:
  hypecli spot-balances --address <ADDRESS>

Query Morpho Position:
  hypecli morpho-position --address <ADDRESS>

Query Morpho APY:
  hypecli morpho-apy --market <MARKET_ID>

Query Morpho Vault APY:
  hypecli morpho-vault-apy --vault <VAULT_ADDRESS>

ORDER COMMANDS
--------------

Place a Limit Order:
  hypecli order limit \
    --chain mainnet \
    --private-key <HEX> \
    --asset BTC \
    --side buy \
    --price 50000 \
    --size 0.1 \
    --tif gtc

  Arguments:
    --asset <NAME>       Asset name (see Asset Name Formats above)
    --side <buy|sell>    Order direction
    --price <DECIMAL>    Limit price
    --size <DECIMAL>     Order size in base asset units
    --tif <gtc|alo|ioc>  Time-in-force (default: gtc)
                         gtc = Good Till Cancel
                         alo = Add Liquidity Only (maker-only)
                         ioc = Immediate or Cancel
    --reduce-only        Optional flag to only reduce existing position
    --cloid <HEX>        Optional client order ID (16 bytes hex)

Place a Market Order:
  hypecli order market \
    --chain mainnet \
    --private-key <HEX> \
    --asset ETH \
    --side buy \
    --size 0.1 \
    --slippage-price 3100

  Arguments:
    --asset <NAME>              Asset name
    --side <buy|sell>           Order direction
    --size <DECIMAL>            Order size
    --slippage-price <DECIMAL>  Worst acceptable fill price
    --reduce-only               Optional flag
    --cloid <HEX>               Optional client order ID

Cancel Order (by OID or CLOID):
  # Cancel by OID (exchange-assigned order ID)
  hypecli order cancel \
    --chain mainnet \
    --private-key <HEX> \
    --asset BTC \
    --oid 123456789

  # Cancel by CLOID (client-assigned order ID)
  hypecli order cancel \
    --chain mainnet \
    --private-key <HEX> \
    --asset BTC \
    --cloid 0x0123456789abcdef0123456789abcdef

  Arguments:
    --asset <NAME>    Asset name the order belongs to
    --oid <NUMBER>    Exchange-assigned order ID (use this OR --cloid)
    --cloid <HEX>     Client-assigned order ID, 32 hex chars (use this OR --oid)

MULTI-SIG COMMANDS
------------------

Convert to Multi-Sig User:
  hypecli to-multisig \
    --chain mainnet \
    --private-key <HEX> \
    --authorized-user <ADDR1> \
    --authorized-user <ADDR2> \
    --threshold 2

Multi-Sig Sign (participates via P2P gossip network):
  hypecli multisig sign \
    --chain mainnet \
    --private-key <HEX> \
    --multi-sig-addr <MULTISIG_ADDRESS>

Multi-Sig Send Asset:
  hypecli multisig send-asset \
    --chain mainnet \
    --private-key <HEX> \
    --multi-sig-addr <MULTISIG_ADDRESS> \
    --destination <RECIPIENT> \
    --token <TOKEN_NAME> \
    --amount <AMOUNT>

Multi-Sig Update Configuration:
  hypecli multisig update \
    --chain mainnet \
    --private-key <HEX> \
    --multi-sig-addr <MULTISIG_ADDRESS> \
    --authorized-user <ADDR1> \
    --authorized-user <ADDR2> \
    --threshold 2

Convert Multi-Sig to Normal User:
  hypecli multisig convert-to-normal-user \
    --chain mainnet \
    --private-key <HEX> \
    --multi-sig-addr <MULTISIG_ADDRESS>

EXAMPLE WORKFLOWS
-----------------

Workflow 1: Place and Cancel a Limit Order
  # 1. Check available markets
  hypecli perps

  # 2. Place a limit buy order for BTC perpetual
  hypecli order limit \
    --chain mainnet \
    --private-key 0xYOUR_KEY \
    --asset BTC \
    --side buy \
    --price 45000 \
    --size 0.01

  # 3. Cancel the order using the returned OID or CLOID
  hypecli order cancel \
    --chain mainnet \
    --private-key 0xYOUR_KEY \
    --asset BTC \
    --oid <RETURNED_OID>

Workflow 2: Market Sell with Slippage Protection
  hypecli order market \
    --chain mainnet \
    --private-key 0xYOUR_KEY \
    --asset ETH \
    --side sell \
    --size 0.1 \
    --slippage-price 2900

Workflow 3: Trade on Spot Market
  hypecli order limit \
    --chain mainnet \
    --private-key 0xYOUR_KEY \
    --asset PURR/USDC \
    --side buy \
    --price 0.05 \
    --size 1000

Workflow 4: Trade on HIP3 DEX
  hypecli order limit \
    --chain mainnet \
    --private-key 0xYOUR_KEY \
    --asset xyz:BTC \
    --side buy \
    --price 45000 \
    --size 0.01

Workflow 5: Using Foundry Keystore
  hypecli order limit \
    --chain mainnet \
    --keystore my-trading-key \
    --asset BTC \
    --side buy \
    --price 45000 \
    --size 0.01
  # Password will be prompted interactively

SEND COMMANDS (Free Asset Transfers)
-------------------------------------

Hyperliquid allows FREE asset transfers with no gas fees. Use the send command
to transfer tokens between accounts, balances, DEXes, and subaccounts.

Send Tokens Between Accounts:
  hypecli send \
    --chain mainnet \
    --private-key <HEX> \
    --token USDC \
    --amount 100 \
    --destination 0x1234...

  Arguments:
    --token <SYMBOL>           Token to send (e.g., USDC, HYPE, PURR)
    --amount <DECIMAL>         Amount to send
    --destination <ADDRESS>    Recipient address (optional, defaults to self)
    --from <LOCATION>          Source: "perp", "spot", or HIP-3 DEX name (default: perp)
    --to <LOCATION>            Destination: "perp", "spot", or HIP-3 DEX name (default: perp)
    --from-subaccount <NAME>   Source subaccount name

Transfer Between Your Own Balances (Perp <-> Spot):
  # Move USDC from perp to spot balance
  hypecli send \
    --chain mainnet \
    --private-key <HEX> \
    --token USDC \
    --amount 100 \
    --from perp \
    --to spot

  # Move HYPE from spot to perp balance
  hypecli send \
    --chain mainnet \
    --private-key <HEX> \
    --token HYPE \
    --amount 50 \
    --from spot \
    --to perp

Send to Another User:
  # Send USDC to another user's perp balance
  hypecli send \
    --chain mainnet \
    --private-key <HEX> \
    --token USDC \
    --amount 100 \
    --destination 0xRECIPIENT_ADDRESS

  # Send HYPE from your spot to another user's spot
  hypecli send \
    --chain mainnet \
    --private-key <HEX> \
    --token HYPE \
    --amount 50 \
    --from spot \
    --to spot \
    --destination 0xRECIPIENT_ADDRESS

Transfer Between DEXes (HIP-3):
  # Transfer from perp to a HIP-3 DEX
  hypecli send \
    --chain mainnet \
    --private-key <HEX> \
    --token USDC \
    --amount 100 \
    --from perp \
    --to xyz

  # Transfer between two HIP-3 DEXes
  hypecli send \
    --chain mainnet \
    --private-key <HEX> \
    --token USDC \
    --amount 100 \
    --from abc \
    --to xyz

Send From Subaccount:
  hypecli send \
    --chain mainnet \
    --private-key <HEX> \
    --token USDC \
    --amount 100 \
    --from-subaccount my-sub \
    --destination 0xRECIPIENT

SUBSCRIBE COMMANDS (Real-time WebSocket Data)
---------------------------------------------

Subscribe commands use the same unified asset format as order commands:
  - "BTC" for BTC perpetual
  - "PURR/USDC" for PURR spot market
  - "xyz:BTC" for BTC perpetual on xyz HIP3 DEX

Subscribe to Trades:
  hypecli subscribe trades --asset BTC
  hypecli subscribe trades --asset PURR/USDC --format json
  hypecli subscribe trades --asset xyz:BTC

Subscribe to Best Bid/Offer (BBO):
  hypecli subscribe bbo --asset BTC
  hypecli subscribe bbo --asset PURR/USDC --format json

Subscribe to Order Book (L2):
  hypecli subscribe orderbook --asset BTC
  hypecli subscribe orderbook --asset PURR/USDC --depth 20

Subscribe to Candles (OHLCV):
  hypecli subscribe candles --asset BTC --interval 1m
  hypecli subscribe candles --asset PURR/USDC --interval 15m --format json

  Available intervals: 1m, 3m, 5m, 15m, 30m, 1h, 2h, 4h, 8h, 12h, 1d, 3d, 1w, 1M

Subscribe to All Mid Prices:
  hypecli subscribe all-mids
  hypecli subscribe all-mids --filter BTC,ETH
  hypecli subscribe all-mids --dex hyperliquid

Subscribe to Order Updates (requires user address):
  hypecli subscribe order-updates --user 0x1234...

Subscribe to Fills (requires user address):
  hypecli subscribe fills --user 0x1234...

Common Options:
  --chain <mainnet|testnet>  Target chain (default: mainnet)
  --format <pretty|json>     Output format (default: pretty)

Workflow 6: Monitor BTC Perpetual Trades
  hypecli subscribe trades --asset BTC

Workflow 7: Monitor Spot Order Book Depth
  hypecli subscribe orderbook --asset PURR/USDC --depth 10

Workflow 8: Stream HIP3 DEX Candle Data as JSON
  hypecli subscribe candles --asset xyz:BTC --interval 5m --format json

ERROR HANDLING
--------------
Common error scenarios:
  - "Order operations require a private key or keystore" - Ledger not supported for orders
  - "keystore doesn't exist" - Check ~/.foundry/keystores/ for available keystores
  - "CLOID must be exactly 16 bytes" - Ensure CLOID is 32 hex characters
  - "Perpetual market 'X' not found" - Use `hypecli perps` to list valid market names
  - "Spot market 'X/Y' not found" - Use `hypecli spot` to list valid spot pairs
  - "HIP3 DEX 'X' not found" - Check DEX name spelling
  - Connection errors - Verify network connectivity and chain selection

OUTPUT FORMAT
-------------
Most commands output human-readable text. Order commands return status information
including order IDs (OID) and client order IDs (CLOID) for successful placements,
which can be used for subsequent cancel operations.
"#
    );
}
