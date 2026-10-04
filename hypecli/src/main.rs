mod account;
mod action;
mod balances;
mod dex;
mod earn;
mod markets;
mod morpho;
mod multisig;
mod orders;
mod orders_list;
mod outcome;
mod positions;
mod prio;
mod send;
mod subscribe;
mod to_multisig;
mod trezor;
mod twap;
mod utils;
mod vault;

use account::AccountCmd;
use balances::BalanceCmd;
use clap::{Args, Parser};
use dex::DexCmd;
use earn::EarnCmd;
use hypersdk::hypercore::Chain;
use markets::{DexesCmd, PerpsCmd, SpotCmd};
use morpho::{MorphoApyCmd, MorphoPositionCmd, MorphoVaultApyCmd};
use multisig::{MultiSigCmd, ToNormalUserCmd};
use orders::OrderCmd;
use orders_list::OrdersCmd;
use outcome::OutcomeCmd;
use positions::PositionsCmd;
use prio::PrioCmd;
use send::SendCmd;
use subscribe::SubscribeCmd;
use to_multisig::ToMultiSigCmd;
use twap::TwapCmd;
use vault::VaultCmd;

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
    /// Account management (create and list keystores)
    #[command(subcommand)]
    Account(AccountCmd),
    /// Query all balances (spot, perp, and DEX) for a user
    Balance(BalanceCmd),
    /// List HIP-3 DEXes
    Dexes(DexesCmd),
    /// HIP-3 DEX deployer operations (inspect, halt, sub-deployers, HIP-3* allowlist)
    #[command(subcommand)]
    Dex(DexCmd),
    /// Hyperliquid Earn: supply, withdraw, and query the borrow/lend reserve
    #[command(subcommand)]
    Earn(EarnCmd),
    /// List perpetual markets
    Perps(PerpsCmd),
    /// List spot markets
    Spot(SpotCmd),
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
    /// Convert a multi-sig user back to a normal user
    ToNormalUser(ToNormalUserCmd),
    /// Order management (place and cancel orders)
    #[command(subcommand)]
    Order(OrderCmd),
    /// HIP-4 outcome token operations (split, merge, negate)
    #[command(subcommand)]
    Outcome(OutcomeCmd),
    /// Subscribe to real-time WebSocket data feeds
    #[command(subcommand)]
    Subscribe(SubscribeCmd),
    /// Send assets between accounts, DEXes, or subaccounts
    Send(SendCmd),
    /// Vault deposit and withdrawal commands
    #[command(subcommand)]
    Vault(VaultCmd),
    /// Query open perpetual positions for a user
    Positions(PositionsCmd),
    /// Query historical orders or trade fills
    #[command(subcommand)]
    Orders(OrdersCmd),
    /// Gossip priority auction: query status or place a bid
    #[command(subcommand)]
    Prio(PrioCmd),
    /// Execute a stealth TWAP as independent market orders
    Twap(TwapCmd),
}

impl Command {
    async fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Account(cmd) => cmd.run().await,
            Self::Balance(cmd) => cmd.run().await,
            Self::Dexes(cmd) => cmd.run().await,
            Self::Dex(cmd) => cmd.run().await,
            Self::Earn(cmd) => cmd.run().await,
            Self::Perps(cmd) => cmd.run().await,
            Self::Spot(cmd) => cmd.run().await,
            Self::MorphoPosition(cmd) => cmd.run().await,
            Self::MorphoApy(cmd) => cmd.run().await,
            Self::MorphoVaultApy(cmd) => cmd.run().await,
            Self::Multisig(cmd) => cmd.run().await,
            Self::ToMultisig(cmd) => cmd.run().await,
            Self::ToNormalUser(cmd) => cmd.run().await,
            Self::Order(cmd) => cmd.run().await,
            Self::Outcome(cmd) => cmd.run().await,
            Self::Subscribe(cmd) => cmd.run().await,
            Self::Send(cmd) => cmd.run().await,
            Self::Vault(cmd) => cmd.run().await,
            Self::Positions(cmd) => cmd.run().await,
            Self::Orders(cmd) => cmd.run().await,
            Self::Prio(cmd) => cmd.run().await,
            Self::Twap(cmd) => cmd.run().await,
        }
    }
}

/// Signer credentials and target chain, shared by signed commands.
#[derive(Args)]
pub struct SignerArgs {
    /// Private key for signing (hex format). Agent (API wallet) keys work
    /// for L1 actions such as orders and outcome operations.
    #[arg(long, conflicts_with_all = ["trezor_path", "trezor_index"])]
    pub private_key: Option<String>,
    /// Foundry keystore.
    #[arg(long, env = "HYPECLI_KEYSTORE", conflicts_with_all = ["trezor_path", "trezor_index"])]
    pub keystore: Option<String>,
    /// Keystore password. Otherwise it'll be prompted.
    #[arg(long, env = "HYPECLI_PASSWORD")]
    pub password: Option<String>,
    #[command(flatten)]
    pub trezor: trezor::TrezorArgs,
    /// Target chain for the operation.
    #[arg(long, default_value = "mainnet")]
    pub chain: Chain,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
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
  - Mainnet  Production Hyperliquid network
  - Testnet  Test network for development

AUTHENTICATION
--------------
Commands that modify state (orders, transfers, etc.) require authentication via SignerArgs:
  --private-key <HEX>   Direct private key (with or without 0x prefix)
  --keystore <NAME>     Foundry keystore name (located in ~/.foundry/keystores/)
  --password <PASS>     Keystore password (prompted if not provided)
  --trezor-index <N>    Select Trezor address m/44'/60'/0'/0/N directly
  --trezor-path <PATH>  Select a full Trezor derivation path directly
  --trezor-scan-limit <N>  Search N addresses locally from a Trezor xpub (default: 10)

Orders, sends, Earn, vault transfers, outcome operations, DEX deployer actions, and priority
bids support Ledger and Trezor. Add --multi-sig-addr <ADDRESS> for multisig and --local to require local signers.
Automated TWAP requires a private key or keystore for continuous signing.

Agent (API wallet) private keys are accepted for L1 actions (orders, TWAP, vault transfers,
outcome operations); the exchange attributes the action to the master account. User-signed
actions (sends, approvals) still require the master key.

ASSET NAME FORMATS
------------------
Order commands use human-readable asset names with automatic index resolution:

  Format        Example         Description
  ──────────────────────────────────────────────────────────
  SYMBOL        BTC, ETH        Perpetual on Hyperliquid DEX
  BASE/QUOTE    PURR/USDC       Spot market
  dex:SYMBOL    xyz:BTC         Perpetual on HIP3 DEX

Use `hypecli perps` or `hypecli spot` to list available markets.
Use `hypecli dexes` to list available HIP-3 DEXes.

ACCOUNT COMMANDS
----------------

Create a New Keystore:
  hypecli account create --name <KEYSTORE_NAME>
  # Password will be prompted interactively
Or alternatively you can specify the password
  hypecli account create --name <KEYSTORE_NAME> --password <PASSWORD>

List Available Keystores:
  hypecli account list

Keystores are stored in ~/.foundry/keystores/ and are compatible with Foundry.

QUERY COMMANDS (No Authentication Required)
-------------------------------------------

List HIP-3 DEXes:
  hypecli dexes

  Lists all available HIP-3 perpetual DEXes by name.

List Perpetual Markets:
  hypecli perps
  hypecli perps --dex <DEX_NAME>

  Options:
  --dex <NAME>  Query markets from a specific HIP-3 DEX

List Spot Markets:
  hypecli spot

Query All Balances (Spot, Perp, All DEXes):
  hypecli balance <ADDRESS>
  hypecli balance <ADDRESS> --format table
  hypecli balance <ADDRESS> --format json
  hypecli balance <ADDRESS> --skip-hip3

  Options:
  --format <pretty|table|json>  Output format (default: pretty)
  --skip-hip3                   Skip querying HIP-3 DEX balances

  Output formats:
  - pretty (default): Human-readable indented output
  - table: Tab-aligned columns for terminal viewing
  - json: Structured JSON for programmatic consumption

  Shows:
  - Spot balances (coin, hold, total)
  - Perp account (account value, margin used, withdrawable, positions)
  - All HIP-3 DEX balances (unless --skip-hip3 is set)

Query Morpho Position:
  hypecli morpho-position --address <ADDRESS>

Query Morpho APY:
  hypecli morpho-apy --market <MARKET_ID>

Query Morpho Vault APY:
  hypecli morpho-vault-apy --vault <VAULT_ADDRESS>

Query Open Positions:
  hypecli positions <ADDRESS>
  hypecli positions <ADDRESS> --format table
  hypecli positions <ADDRESS> --coin BTC --format json

  Options:
  --coin <SYMBOL>       Filter to a specific coin (e.g., BTC, ETH)
  --dex <NAME>          Query a HIP-3 DEX instead of the default perp DEX
  --format <pretty|table|json>  Output format (default: pretty)

  Shows size, side, entry price, unrealized PnL, leverage, liquidation price,
  margin used, and cumulative funding for each open position.

Query Historical Orders:
  hypecli orders list <ADDRESS>
  hypecli orders list <ADDRESS> --coin BTC --format json

Query Trade Fills:
  hypecli orders fills <ADDRESS>
  hypecli orders fills <ADDRESS> --coin BTC --format table

  Common options for both:
  --coin <SYMBOL>             Filter by asset
  --format <pretty|table|json>

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

OUTCOME COMMANDS (HIP-4)
------------------------

Outcome markets are fully collateralized binary contracts. The quote token can be
split into one share of each side of an outcome, and matching shares can be merged
back. Outcome and question IDs come from `hypecli outcome list`.

List Outcomes and Questions:
  hypecli outcome list
  hypecli outcome list --chain testnet

Split Quote Token into Outcome Shares:
  hypecli outcome split \
    --chain mainnet \
    --private-key <HEX> \
    --outcome 12 \
    --amount 100

Merge Outcome Shares Back into the Quote Token:
  hypecli outcome merge \
    --chain mainnet \
    --private-key <HEX> \
    --outcome 12 \
    --amount 100

  Omit --amount to merge the maximum available.

Merge a Question's Full Outcome Set:
  hypecli outcome merge-question \
    --chain mainnet \
    --private-key <HEX> \
    --question 3

  Merges a full set of mutually-exclusive outcomes within a question back into
  the quote token. Omit --amount to merge the maximum available.

Negate an Outcome within a Question:
  hypecli outcome negate \
    --chain mainnet \
    --private-key <HEX> \
    --question 3 \
    --outcome 12 \
    --amount 50

  Converts shares of one outcome into the complementary basket (the "No" of
  that outcome) within a categorical question.

DEX COMMANDS (HIP-3 DEPLOYERS)
------------------------------

Signed commands must come from the DEX's deployer or a sub-deployer it has
granted that action. HIP-3* venues (allowlisted, testnet-only) add allow,
disallow, reduce-only, and cancel-all, which act on one user of the venue.

Show a DEX's Configuration:
  hypecli dex info --dex xyz
  hypecli dex info --dex test --chain testnet

  Prints the deployer, oracle updater, fee recipient, net deposit, the
  sub-deployers of each delegated action, and per-coin OI caps and funding
  settings. HIP-3* grants print as hip3Star:<operation>.

Halt or Resume Trading on a Coin:
  hypecli dex halt \
    --chain mainnet \
    --private-key <HEX> \
    --coin xyz:SP500

  hypecli dex resume \
    --chain mainnet \
    --private-key <HEX> \
    --coin xyz:SP500

  --coin needs the DEX prefix.

Grant or Revoke a Sub-Deployer Permission:
  hypecli dex sub-deployer \
    --chain mainnet \
    --private-key <HEX> \
    --dex xyz \
    --user <ADDRESS> \
    --permission setOracle

  --permission takes a perpDeploy action such as setOracle or haltTrading, or
  hip3Star:<operation> for a HIP-3* operation, e.g. hip3Star:modifyApproval.
  Add --revoke to remove the permission.

Add or Remove a HIP-3* User (testnet-only):
  hypecli dex allow \
    --chain testnet \
    --private-key <HEX> \
    --dex test \
    --user <ADDRESS>

  hypecli dex disallow takes the same arguments. Removing a user clears their
  flags; approving them again starts them with the defaults.

Restrict a HIP-3* User to Reducing Positions (testnet-only):
  hypecli dex reduce-only \
    --chain testnet \
    --private-key <HEX> \
    --dex test \
    --user <ADDRESS>

  Add --off to restore full trading.

Cancel a HIP-3* User's Orders and TWAPs (testnet-only):
  hypecli dex cancel-all \
    --chain testnet \
    --private-key <HEX> \
    --dex test \
    --user <ADDRESS>

Show a User's HIP-3* State:
  hypecli dex star-state <ADDRESS> --chain testnet

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
  hypecli send \
    --chain mainnet \
    --private-key <HEX> \
    --multi-sig-addr <MULTISIG_ADDRESS> \
    --destination <RECIPIENT> \
    --token <TOKEN_NAME> \
    --amount <AMOUNT>

  Add --local to require all signatures locally. Use --from and --to to select
  balances or DEXes. Omit --destination for transfers within the multisig account.
  The older `hypecli multisig send-asset` command remains supported.

Multi-Sig Update Configuration:
  hypecli multisig update \
    --chain mainnet \
    --private-key <HEX> \
    --multi-sig-addr <MULTISIG_ADDRESS> \
    --authorized-user <ADDR1> \
    --authorized-user <ADDR2> \
    --threshold 2

Convert Multi-Sig to Normal User:
  hypecli to-normal-user \
    --chain mainnet \
    --private-key <HEX> \
    --multi-sig-addr <MULTISIG_ADDRESS>

  The older `hypecli multisig convert-to-normal-user` command remains supported.

GOSSIP PRIORITY AUCTION COMMANDS
--------------------------------

Query Auction Status:
  hypecli prio status

  Shows current winning prices, time remaining, and winners for all 5 slots (0–4).
  Slot 0 = highest priority (~10ms faster than slot 1, etc.).

Place a Priority Bid:
  hypecli prio bid \
    --chain mainnet \
    --private-key <HEX> \
    --max 0.5 \
    --ip 203.0.113.42 \
    --slot 0

  Arguments:
    --max <HYPE>       Maximum bid in HYPE units (not wei). 1 HYPE = 1e18 wei.
    --ip <ADDRESS>     IP address to receive prioritized gossip data.
    --slot <0-4>       Slot index (default: 0, highest priority).

  The winning bid amount is deducted from your spot HYPE balance and burned.
  Use `hypecli prio status` first to decide how much to bid.

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

VAULT COMMANDS
--------------

Deposit USDC into a vault:
  hypecli vault deposit \
    --chain mainnet \
    --private-key <HEX> \
    --vault <VAULT_ADDRESS> \
    --amount 100

Withdraw USDC from a vault:
  hypecli vault withdraw \
    --chain mainnet \
    --private-key <HEX> \
    --vault <VAULT_ADDRESS> \
    --amount 100

  Arguments:
    --vault <ADDRESS>    Vault address to deposit into or withdraw from
    --amount <DECIMAL>   Amount of USDC

EARN COMMANDS (Borrow/Lend Reserve)
-----------------------------------

Select an Earn reserve with --token USDC or --token USDT0. The default is USDC.
Symbols are case-insensitive, and numeric token indexes are also accepted.

Supply USDC into the Earn reserve (omit --amount for the maximum available):
  hypecli earn supply \
    --chain mainnet \
    --private-key <HEX> \
    --amount 100

Withdraw supplied USDC (omit --amount for the full position):
  hypecli earn withdraw \
    --chain mainnet \
    --private-key <HEX> \
    --amount 100

Query reserve rates and your position:
  hypecli earn status --user <ADDRESS>

Supply USDC from a multisig account:
  hypecli earn supply \
    --multi-sig-addr <MULTISIG_ADDRESS> \
    --keystore my-wallet \
    --token USDC \
    --amount 100

Other signers join using the printed `hypecli multisig sign` command.
Add --local to require all signatures locally. Withdraw supports the same flags.
For multisig positions, pass the multisig address to `earn status --user`.

  Arguments:
    --amount <DECIMAL>   Amount to supply or withdraw (omit = max)
    --token <SYMBOL_OR_INDEX>  Reserve token symbol or index (default USDC)
    --user <ADDRESS>     User address for status
    --multi-sig-addr <ADDRESS>  Multisig account for supply or withdraw
    --local             Collect only local signatures (requires --multi-sig-addr)

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
  - Automated TWAP requires --private-key or --keystore
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
