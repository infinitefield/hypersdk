# Hypersdk Examples

This directory contains comprehensive examples demonstrating all major SDK features. Examples are organized by complexity and use case.

## Setup

Most examples require a private key set via environment variable:

```bash
export PRIVATE_KEY="your_private_key_here"
```

Or create a `.env` file in the project root:

```bash
PRIVATE_KEY=your_private_key_here
```

## Running Examples

```bash
# HyperCore examples
cargo run --example list_markets
cargo run --example send_order

# Morpho examples
cargo run --example morpho_highest_apy

# Uniswap examples
cargo run --example uniswap_pools_created
```

---

## HyperCore Examples

### Beginner - Market Data & Queries

Start here if you're new to the SDK. These examples show read-only operations.

| Example | Description | Requires Key |
|---------|-------------|--------------|
| `list-markets` | List all perpetual markets with details (leverage, tick size) | No |
| `list-tokens` | List all spot tokens with metadata | No |
| `list-hip3` | Query HIP-3 DEXes and their perpetual markets | No |
| `websocket` | Subscribe to real-time trades, order books, and user events | No (read-only) |

**Try first:** `cargo run --example list_markets`

### Intermediate - Trading & Transfers

These examples require a private key and perform state-changing operations.

| Example | Description | Requires Key |
|---------|-------------|--------------|
| `send_order` | Place a single limit order on a perpetual market | Yes |
| `send_usd` | Send USDC from perpetual balance to another address | Yes |
| `transfer_to_evm` | Transfer assets from HyperCore to HyperEVM | Yes |
| `transfer_from_evm` | Transfer assets from HyperEVM to HyperCore | Yes |
| `transfer_to_perps` | Move assets from spot to perpetual balance | Yes |
| `transfer_to_spot` | Move assets from perpetual to spot balance | Yes |

**Try next:** `cargo run --example send_order` (after funding your testnet account)

### Advanced - Multi-Signature & Complex Flows

These examples demonstrate advanced patterns for production systems.

| Example | Description | Requires Key |
|---------|-------------|--------------|
| `multisig_order` | Place orders with multiple signers (vault/custody pattern) | Yes (multiple) |
| `multisig_send_usd` | Multi-sig USDC transfer | Yes (multiple) |
| `multisig_send_asset` | Multi-sig asset transfer between DEXes | Yes (multiple) |
| `buy_and_transfer` | Complex flow: place order, wait for fill, transfer to EVM | Yes |

**Production pattern:** See `multisig_order` for how institutional systems manage custody

---

## HyperEVM - Morpho Examples

Morpho is a lending protocol on HyperEVM. These examples show how to query lending rates and vault performance.

| Example | Description | Requires Key |
|---------|-------------|--------------|
| `morpho_highest_apy` | Find vaults with highest APY for yield optimization | No |
| `morpho_supply_apy` | Query supply APY for specific markets | No |
| `morpho_borrow_apy` | Query borrow APY for specific markets | No |
| `morpho_vault_apy` | Get detailed vault APY breakdown | No |
| `morpho_vault_performance` | Analyze vault performance metrics over time | No |
| `morpho_create_market_events` | Subscribe to new Morpho market creation events | No |

**Use case:** Integrate these into lending aggregators or yield farming bots

```bash
cargo run --example morpho_highest_apy
```

---

## HyperEVM - Uniswap Examples

Uniswap V3 integration for querying pools and tracking liquidity positions.

| Example | Description | Requires Key |
|---------|-------------|--------------|
| `uniswap_pools_created` | Monitor new pool creation events | No |
| `uniswap_prjx_flows` | Track PRJX token flows and liquidity | No |

**Use case:** Market making bots, liquidity analytics, DEX aggregators

```bash
cargo run --example uniswap_pools_created
```

---

## Example Output Samples

### list_markets
```
BTC: max leverage 50x, tick size 1, sz_decimals 5
ETH: max leverage 50x, tick size 0.1, sz_decimals 4
...
```

### websocket
```
Trade: Buy @ 93231.0 size 0.5
Order book update for BTC: 50 bids, 50 asks
User fill: +0.1 BTC @ 93230.0
```

### morpho_highest_apy
```
Vault: USDC Optimizer
  Supply APY: 8.45%
  Net APY: 8.23%
  TVL: $1,234,567
```

---

## Common Patterns

### Price Rounding
All order prices must be rounded to valid tick sizes:

```rust
let client = hypercore::mainnet();
let markets = client.perps().await?;
let btc = markets.iter().find(|m| m.name == "BTC").unwrap();

// Round price to valid tick
let rounded = btc.round_price(dec!(93231.23)); // -> 93231
```

See `send_order.rs` for complete example.

### Error Handling
All examples use `anyhow::Result` for simple error propagation:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Your code here
    Ok(())
}
```

### WebSocket Reconnection
The `websocket.rs` example shows basic subscription, but production systems should implement:
- Automatic reconnection with exponential backoff
- Subscription state management across reconnects
- Message deduplication

---

## Troubleshooting

### "Private key not found"
Set the `PRIVATE_KEY` environment variable or create a `.env` file.

### "Order rejected: price not on tick"
Use `market.round_price()` to round prices to valid tick sizes before submitting orders.

### "Insufficient balance"
Fund your testnet account at: https://app.hyperliquid-testnet.xyz/faucet

### "Connection timeout"
Ensure you're using the correct network (mainnet vs testnet). Check `hypercore::mainnet()` vs `hypercore::testnet()`.

---

## Next Steps

1. **Start with read-only examples** (`list_markets`, `websocket`)
2. **Get testnet funds** from the faucet
3. **Try a simple trade** with `send_order`
4. **Explore advanced patterns** like multi-sig and cross-chain transfers
5. **Build your bot** using WebSocket subscriptions for real-time data

## Documentation

- [Full API Documentation](https://docs.rs/hypersdk)
- [Hyperliquid Docs](https://hyperliquid.gitbook.io/hyperliquid-docs/)
- [CLAUDE.md](../CLAUDE.md) - Developer guide for working with this codebase

## Contributing

Found a bug in an example? Have a use case we don't cover? Open an issue or PR!
