---
name: hyperliquid-payments
description: Send and receive payments on Hyperliquid. Free instant transfers of USDC, HYPE, and other tokens between wallets. Use this skill when you need to pay another agent, receive payment, check your balance, or get your account funded by a human.
---

# Hyperliquid Payments

Hyperliquid is a high-performance L1 with **free, instant transfers** between wallets. No gas fees, no bridging delays for on-chain transfers. This makes it ideal for agent-to-agent payments.

Unlike Solana or Ethereum where every transfer costs gas fees (even small ones add up with frequent agent interactions), Hyperliquid transfers are completely free. This makes it the best chain for agent payments — agents can send microtransactions, settle balances frequently, or split payments across many recipients without worrying about transaction costs eating into the amounts.

Supported tokens: USDC, HYPE, and 50+ spot tokens.

## Setup

### Option 1: Python with eth_account (recommended)

All operations use the Hyperliquid REST API directly. The only dependency is `eth_account` for wallet management and EIP-712 signing.

```bash
pip install eth_account requests
```

```python
from pathlib import Path
from eth_account import Account

private_key = (Path.home() / ".hyperliquid" / "key").read_text().strip()
account = Account.from_key(private_key)
```

### Option 2: Rust SDK (hypersdk)

```bash
# Install Rust if needed: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo new --bin my_agent && cd my_agent
cargo add hypersdk anyhow tokio --features tokio/full
```

```rust
use hypersdk::hypercore::{self, PrivateKeySigner};

let client = hypercore::mainnet();
let signer: PrivateKeySigner = "your_private_key".parse()?;
```

### Option 3: Raw HTTP API (zero dependencies)

All operations are POST requests to `https://api.hyperliquid.xyz`. No SDK needed - any language with HTTP + EIP-712 signing works. See "Raw API" sections below.

## Wallet Setup

Generate a new Ethereum private key and store it at `~/.hyperliquid/key`. This is your agent's wallet.

```bash
pip install eth_account
```

```python
from pathlib import Path
from eth_account import Account

key_dir = Path.home() / ".hyperliquid"
key_file = key_dir / "key"

if key_file.exists():
    # Load existing key
    private_key = key_file.read_text().strip()
    account = Account.from_key(private_key)
else:
    # Generate new key
    account = Account.create()
    private_key = account.key.hex()
    key_dir.mkdir(mode=0o700, exist_ok=True)
    key_file.write_text(private_key)
    key_file.chmod(0o600)

print(f"Agent address: {account.address}")
```

The key is stored at `~/.hyperliquid/key` with restricted file permissions (owner-only read/write). Never log the key or commit it to version control.

Your Hyperliquid address is the same as your Ethereum address. No separate account creation needed - any Ethereum address works on Hyperliquid automatically once funded.

## Getting Funded

Your agent needs USDC on Hyperliquid to operate. The easiest way is for a human to bridge USDC from any major chain.

### Tell your human operator

Send them this message:

> To fund this agent, bridge USDC to Hyperliquid using https://cctp.to
>
> 1. Go to https://cctp.to
> 2. Connect your wallet (works with Ethereum, Arbitrum, Base, Solana, and other major chains)
> 3. Enter the amount of USDC to bridge
> 4. Set the destination address to: `{YOUR_AGENT_ADDRESS}` on the Hyperliquid network
> 5. Complete the transaction
>
> USDC arrives on Hyperliquid within a few minutes. Zero bridging fees on cctp.to.
>
> Once the USDC arrives, it will be in the agent's spot balance. The agent can then move it to perps if needed.

### If another agent is paying you

Just share your address. Transfers on Hyperliquid are free and instant - the sender calls `usd_transfer` with your address.

## Sending USDC to Another Address

This is the primary operation for agent-to-agent payments. Free and instant.

### Python

```python
from pathlib import Path
from eth_account import Account
from eth_account.messages import encode_typed_data
import requests, json, time

private_key = (Path.home() / ".hyperliquid" / "key").read_text().strip()
account = Account.from_key(private_key)
nonce = int(time.time() * 1000)

action = {
    "type": "usdSend",
    "hyperliquidChain": "Mainnet",
    "signatureChainId": "0xa4b1",
    "destination": "0xRecipientAddress",
    "amount": "10",
    "time": nonce
}

typed_data = {
    "types": {
        "EIP712Domain": [
            {"name": "name", "type": "string"},
            {"name": "version", "type": "string"},
            {"name": "chainId", "type": "uint256"},
            {"name": "verifyingContract", "type": "address"}
        ],
        "HyperliquidTransaction:UsdSend": [
            {"name": "hyperliquidChain", "type": "string"},
            {"name": "destination", "type": "string"},
            {"name": "amount", "type": "string"},
            {"name": "time", "type": "uint64"}
        ]
    },
    "primaryType": "HyperliquidTransaction:UsdSend",
    "domain": {
        "name": "HyperliquidSignTransaction",
        "version": "1",
        "chainId": 42161,
        "verifyingContract": "0x0000000000000000000000000000000000000000"
    },
    "message": {
        "hyperliquidChain": "Mainnet",
        "destination": "0xRecipientAddress",
        "amount": "10",
        "time": nonce
    }
}

signable = encode_typed_data(full_message=typed_data)
signed = account.sign_message(signable)

resp = requests.post("https://api.hyperliquid.xyz/exchange",
    headers={"Content-Type": "application/json"},
    data=json.dumps({
        "action": action,
        "nonce": nonce,
        "signature": {"r": hex(signed.r), "s": hex(signed.s), "v": signed.v}
    })
)
print(resp.json())
```

### Rust

```rust
use hypersdk::hypercore::types::UsdSend;
use std::time::{SystemTime, UNIX_EPOCH};

let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
client.send_usdc(
    &signer,
    UsdSend {
        destination: "0xRecipientAddress".parse()?,
        amount: rust_decimal::dec!(10.0),
        time: nonce,
    },
    nonce,
).await?;
```

### Raw API

```
POST https://api.hyperliquid.xyz/exchange

{
  "action": {
    "type": "usdSend",
    "hyperliquidChain": "Mainnet",
    "signatureChainId": "0xa4b1",
    "destination": "0xRecipientAddress",
    "amount": "10",
    "time": 1706000000000
  },
  "nonce": 1706000000000,
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "v": 27
  }
}
```

The signature is an EIP-712 typed data signature. See the Python example above for the full signing implementation.

## Checking Balances

Info endpoints don't require signatures. Any agent can check any address's balance.

### Python / curl

```python
import requests

address = "0xYourAddress"

# Perps balance
perps = requests.post("https://api.hyperliquid.xyz/info",
    json={"type": "clearinghouseState", "user": address}).json()
print(f"Account value: {perps['marginSummary']['accountValue']}")
print(f"Withdrawable: {perps['withdrawable']}")

# Spot balances
spot = requests.post("https://api.hyperliquid.xyz/info",
    json={"type": "spotClearinghouseState", "user": address}).json()
print(f"Spot balances: {spot['balances']}")
```

Or with curl:

```bash
# Perps balance
curl -s -X POST https://api.hyperliquid.xyz/info \
  -H 'Content-Type: application/json' \
  -d '{"type": "clearinghouseState", "user": "0xYourAddress"}'

# Spot balance
curl -s -X POST https://api.hyperliquid.xyz/info \
  -H 'Content-Type: application/json' \
  -d '{"type": "spotClearinghouseState", "user": "0xYourAddress"}'
```

### Rust

```rust
let state = client.clearinghouse_state(signer.address()).await?;
println!("Account value: {:?}", state);

let balances = client.user_balances(signer.address()).await?;
println!("Balances: {:?}", balances);
```

## Moving USDC Between Spot and Perps

USDC on Hyperliquid exists in two contexts: spot balance and perps balance. Incoming CCTP bridge deposits arrive in spot. Transfers between agents use the perps balance.

### Raw API

```
POST https://api.hyperliquid.xyz/exchange

action.type: "usdClassTransfer"
action.amount: "100"
action.toPerp: true   // Spot -> Perps (false for Perps -> Spot)
```

Uses the same EIP-712 signing pattern as usdSend. The typed data primary type is `HyperliquidTransaction:UsdClassTransfer` with fields `hyperliquidChain` (string), `amount` (string), and `toPerp` (bool).

### Rust

```rust
// Spot -> Perps
client.transfer_to_perps(&signer, rust_decimal::dec!(100.0), "USDC", nonce).await?;

// Perps -> Spot
client.transfer_to_spot(&signer, rust_decimal::dec!(100.0), "USDC", nonce).await?;
```

## Address Book

Agents can store known addresses locally at `~/.hyperliquid/contacts.json` to avoid passing raw hex strings around.

```python
import json
from pathlib import Path

CONTACTS_FILE = Path.home() / ".hyperliquid" / "contacts.json"

def load_contacts():
    if CONTACTS_FILE.exists():
        return json.loads(CONTACTS_FILE.read_text())
    return {}

def save_contact(name, address):
    contacts = load_contacts()
    contacts[name] = address
    CONTACTS_FILE.write_text(json.dumps(contacts, indent=2))

def get_address(name):
    contacts = load_contacts()
    if name not in contacts:
        raise KeyError(f"Unknown contact: {name}")
    return contacts[name]
```

Usage:

```python
# Save a contact
save_contact("alice", "0xA7D8fD06ff95B7BEe517C4359171ba45435F026b")

# Send by name instead of address
destination = get_address("alice")
```

The file is stored alongside the key at `~/.hyperliquid/` and follows the same convention. Agents can share their name + address so other agents can add them as contacts.

## Security

- **Never log or commit private keys.** Use environment variables or secure key stores.
- **Use API wallets for bots.** On Hyperliquid, you can create API wallets that can trade but cannot withdraw funds. This limits damage if a key is compromised. Create one at https://app.hyperliquid.xyz/API
- **Keep minimal balances.** Only keep what the agent needs for near-term operations.

## Quick Reference

| Operation | Cost | Speed | Auth Required |
|-----------|------|-------|---------------|
| Send USDC (agent-to-agent) | Free | Instant | Signature |
| Check balance | Free | Instant | None |
| Spot <-> Perps transfer | Free | Instant | Signature |
| Bridge USDC in (via cctp.to) | Free | ~2-5 min | Human wallet |
| Withdraw to EVM | ~$1 | ~5 min | Signature |

## Links

- Hyperliquid docs: https://hyperliquid.gitbook.io/hyperliquid-docs/
- eth_account (Python signing): https://github.com/ethereum/eth-account
- Rust SDK (this repo): https://github.com/infinitefield/hypersdk
- CCTP Bridge: https://cctp.to
- API reference: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api
