# hypecli

A command-line interface for interacting with the [Hyperliquid](https://app.hyperliquid.xyz) protocol.

[![Crates.io](https://img.shields.io/crates/v/hypecli.svg)](https://crates.io/crates/hypecli)
[![License: MPL 2.0](https://img.shields.io/badge/License-MPL_2.0-blue.svg)](https://opensource.org/licenses/MPL-2.0)

## Overview

`hypecli` is a lightweight CLI tool built on top of [hypersdk](https://github.com/infinitefield/hypersdk) for quick queries and operations on Hyperliquid. It provides fast access to market data, user balances, and DeFi protocol information without writing custom code.

## Installation

### Quick Install

```bash
curl -fsSL https://raw.githubusercontent.com/infinitefield/hypersdk/main/hypecli/install.sh | sh
```

### From crates.io

```bash
cargo install hypecli
```

### From source

```bash
git clone https://github.com/infinitefield/hypersdk.git
cd hypersdk/hypecli
cargo install --path .
```

## Usage

```bash
hypecli --help
```

### Account Management

Create and manage Foundry-compatible keystores for signing transactions.

```bash
# Create a new keystore with a random private key
hypecli account create --name my-wallet
# You'll be prompted to enter and confirm a password

# List all available keystores
hypecli account list
```

Keystores are stored in `~/.foundry/keystores/` and are compatible with Foundry's `cast` tool. Use the keystore name with `--keystore` in other commands.

### List HIP-3 DEXes

List all available HIP-3 perpetual DEXes.

```bash
hypecli dexes
```

### List Perpetual Markets

List perpetual markets from Hyperliquid or a specific HIP-3 DEX.

```bash
# List all Hyperliquid perpetual markets
hypecli perps

# List perpetual markets from a specific HIP-3 DEX
hypecli perps --dex xyz
```

### List Spot Markets

List all spot trading pairs.

```bash
hypecli spot
```

### Query Balances

Query all balances (spot, perp, and DEX) for a user address.

```bash
# Pretty format (default)
hypecli balance 0x1234567890abcdef1234567890abcdef12345678

# Table format for terminal viewing
hypecli balance 0x1234... --format table

# JSON format for programmatic consumption
hypecli balance 0x1234... --format json

# Skip querying HIP-3 DEX balances (only show spot and perp)
hypecli balance 0x1234... --skip-hip3
```

Shows spot balances, perp account details (account value, margin used, withdrawable, positions), and all HIP-3 DEX balances. Use `--skip-hip3` to skip DEX queries.

### Placing Orders

Place limit or market orders on perpetual markets.

```bash
# Place a limit buy order for 0.1 BTC at $50,000
hypecli order limit \
  --keystore my-wallet \
  --asset BTC \
  --side buy \
  --price 50000 \
  --size 0.1

# Place a market sell order with slippage protection
hypecli order market \
  --keystore my-wallet \
  --asset ETH \
  --side sell \
  --size 1 \
  --slippage-price 3400

# Cancel an order by OID
hypecli order cancel \
  --keystore my-wallet \
  --asset BTC \
  --oid 123456789
```

Time-in-force options: `gtc` (default), `alo` (add liquidity only), `ioc` (immediate or cancel).

### Query Positions

View open perpetual positions for a user address.

```bash
hypecli positions 0x1234567890abcdef1234567890abcdef12345678
```

### Query Orders and Fills

List historical orders or trade fills.

```bash
# List open orders
hypecli orders list 0x1234567890abcdef1234567890abcdef12345678

# List fills
hypecli orders fills 0x1234567890abcdef1234567890abcdef12345678
```

### Sending Assets

Send tokens between accounts, DEXes, or subaccounts.

```bash
# Send USDC to another address
hypecli send \
  --keystore my-wallet \
  --token USDC \
  --amount 100 \
  --destination 0xRecipientAddress...

# Transfer from spot to perp balance
hypecli send \
  --keystore my-wallet \
  --token USDC \
  --amount 100 \
  --from spot \
  --to perp
```

### Vault Deposits and Withdrawals

Deposit into or withdraw from yield vaults.

```bash
# Deposit 100 USDC into a vault
hypecli vault deposit \
  --keystore my-wallet \
  --vault 0xVaultAddress... \
  --amount 100

# Withdraw 50 USDC from a vault
hypecli vault withdraw \
  --keystore my-wallet \
  --vault 0xVaultAddress... \
  --amount 50
```

### Subscribe to WebSocket Feeds

Subscribe to real-time WebSocket data feeds.

```bash
# Subscribe to trades
hypecli subscribe trades BTC

# Subscribe to order book
hypecli subscribe book ETH
```

### Gossip Priority (Dutch Auction)

Hyperliquid's gossip network uses 5 Dutch auction slots (indices 0–4) for read-priority ordering. When you win a slot, your node receives transaction data ~10ms faster per slot level before non-winners see it. All 5 slots reset on a synchronized cycle (~3 minutes).

**How it works:**

| `currentGas`       | Meaning                                              |
|--------------------|------------------------------------------------------|
| not null           | Auction RUNNING at displayed price. You can bid now. |
| null               | Settled, winner set or no bids placed this cycle.   |

When you bid, you pay the **live `currentGas` price** at TX mining time, not your ceiling (`--max`). The difference is refunded automatically. Winning bid amounts are burned from your spot HYPE balance.

Lower slot index = higher priority:

```
Slot 0 -> ~50ms faster than no-bid nodes
Slot 1 -> ~40ms faster
Slot 2 -> ~30ms faster
Slot 3 -> ~20ms faster
Slot 4 -> ~10ms faster
```

**Check current prices:**

```bash
hypecli prio status
```

Output shows when the current cycle started and live prices for all slots:

```
started 2026-04-20 14:45:00 UTC

Slot          Start      Current      End/Min
------------------------------------------------
0               1.0      0.50157            -
1               1.0        0.1000            -
2               0.1        0.1000            -
...
```

`Start` is the opening price, `Current` is the live decaying Dutch price, `End/Min` is the floor (set after settlement).

**Place a bid:**

```bash
hypecli prio bid \
  --keystore if_dev \
  --ip 52.196.250.75 \
  --max 1 \
  --slot 0
```

If `currentGas == 0.50157`, you pay exactly `0.50157 HYPE` at mining time. Any amount between `currentGas` and `--max` is refunded. If `currentGas >= --max`, bidding is skipped.

### Features

#### Multi-Signature Transactions (P2P)

Coordinate multi-signature transactions using decentralized peer-to-peer gossip, without relying on a centralized server.

##### Initiating an Asset Transfer

The initiator creates a transaction proposal and waits for authorized signers to connect and sign:

```bash
hypecli multisig send-asset \
  --multi-sig-addr 0xYourMultiSigWallet... \
  --chain Mainnet \
  --to 0xRecipient... \
  --token USDC \
  --amount 100 \
  --keystore my-wallet
```

If no wallet is detected, `hypecli` defaults to a connected Ledger, if any.

**Output:**

```
Using signer 0xSigner1...
Authorized users: [0xSigner1..., 0xSigner2..., 0xSigner3...]

hypecli multisig sign --multi-sig-addr 0xYourMultiSigWallet... --chain Mainnet --connect endpoint...

Authorized 1/2
```

The command displays a connection ticket that other signers can use to connect. It waits until the signature threshold is met, then submits the transaction.

##### Signing a Transaction

Other authorized signers connect to the initiator using the endpoint ticket:

```bash
hypecli multisig sign \
  --multi-sig-addr 0xYourMultiSigWallet... \
  --chain Mainnet \
  --connect endpoint... \
  --keystore another-wallet
```

**Output:**

```
Signer found using 0xSigner2...
Neighbor up: abc123...
SendAsset {
    destination: 0xRecipient...,
    token: "USDC",
    amount: 100,
    ...
}
Accept (y/n)?
```

The signer reviews the transaction details and types `y` to approve or `n` to reject.

**Signer Options:**

You can provide signing credentials via:

- `--private-key 0x...` - Direct private key (hex format)
- `--keystore filename` - Foundry keystore file (prompts for password)
- No flag - Automatically searches connected Ledger devices

For keystores:

```bash
# With password prompt
hypecli multisig sign --keystore my-wallet --connect endpoint...

# With password flag (less secure, visible in history)
hypecli multisig sign --keystore my-wallet --password mypass --connect endpoint...
```

**How P2P Multi-Sig Works:**

1. **Decentralized**: Uses Iroh's gossip protocol for peer-to-peer communication
2. **No Server**: No centralized coordinator required
3. **NAT Traversal**: Supports mDNS and DNS discovery with relay fallback
4. **Secure**: Each signer reviews and cryptographically signs the exact transaction
5. **Threshold**: Collects signatures until the on-chain threshold is met
6. **Privacy**: Ephemeral keys used for P2P connections

**Network Discovery:**

The CLI uses multiple discovery mechanisms:

- **mDNS**: Discovers peers on the local network
- **DNS**: Uses the n0 relay network for discovery
- **Direct Connect**: Uses endpoint tickets for direct peer connections

This allows signers to coordinate from anywhere, even behind NATs or firewalls.

## Documentation

- [hypersdk Documentation](https://docs.rs/hypersdk)
- [Hyperliquid API Docs](https://hyperliquid.gitbook.io/hyperliquid-docs/)

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

Ideas for contributions:

- Morpho supply/borrow operations (deposit, withdraw, borrow, repay)
- Uniswap V3 swap and liquidity operations
- Configuration file support (for default keystore, chain, etc.)
- Interactive/repl mode
- Performance optimizations

## License

This project is licensed under the Mozilla Public License 2.0 - see the [LICENSE](../LICENSE) file for details.

## Support

- GitHub Issues: [Report bugs or request features](https://github.com/infinitefield/hypersdk/issues)
- Documentation: [docs.rs/hypersdk](https://docs.rs/hypersdk)

---

**Note**: This CLI is not officially affiliated with Hyperliquid. It is a community-maintained project built on hypersdk.
