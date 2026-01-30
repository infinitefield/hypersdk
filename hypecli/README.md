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

- New commands for trading operations
- JSON output format support
- Configuration file support
- Interactive mode
- Performance optimizations

## License

This project is licensed under the Mozilla Public License 2.0 - see the [LICENSE](../LICENSE) file for details.

## Support

- GitHub Issues: [Report bugs or request features](https://github.com/infinitefield/hypersdk/issues)
- Documentation: [docs.rs/hypersdk](https://docs.rs/hypersdk)

---

**Note**: This CLI is not officially affiliated with Hyperliquid. It is a community-maintained project built on hypersdk.
