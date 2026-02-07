# hypersdk

A comprehensive Rust SDK for interacting with the [Hyperliquid](https://app.hyperliquid.xyz) protocol.

[![Crates.io](https://img.shields.io/crates/v/hypersdk.svg)](https://crates.io/crates/hypersdk)
[![Documentation](https://docs.rs/hypersdk/badge.svg)](https://docs.rs/hypersdk)
[![License: MPL 2.0](https://img.shields.io/badge/License-MPL_2.0-blue.svg)](https://opensource.org/licenses/MPL-2.0)

> **AI Agents**: Install `hypecli` and check the [`skills/`](skills/) folder for guides on payments, trading, and more. No Rust toolchain required.

## Overview

Hyperliquid is a high-performance decentralized exchange with two main components:

- **HyperCore**: The native L1 chain with perpetual and spot markets
- **HyperEVM**: An Ethereum-compatible layer for DeFi integrations (Morpho, Uniswap, etc.)

This SDK provides:

- Full HyperCore API support (HTTP and WebSocket)
- Trading operations (orders, cancellations, modifications)
- Real-time market data via WebSocket subscriptions
- Asset transfers between perps, spot, and EVM
- HyperEVM contract interactions (Morpho, Uniswap)
- Type-safe EIP-712 signing for all operations
- Accurate price tick rounding for orders
- **HIP-3 support**: Query perpetual markets from multiple DEXes
- **CLI tool** (`hypecli`): Command-line interface for Hyperliquid (will be extended in the future)

## Design Choices

### Core Dependencies

**[alloy](https://alloy.rs/)** - EVM and signature handling

- Used for all EVM interactions and Hyperliquid L1 signatures
- Provides type-safe Ethereum primitives and signing utilities

**[rust_decimal](https://docs.rs/rust_decimal)** - High-precision decimals

- Primary choice for financial calculations requiring precision
- Converts WebSocket string payloads to high-precision decimal numbers
- Can be easily converted to other fixed-point number types
- Note: Some specialized EVM types may require alternative approaches

**[yawc](https://docs.rs/yawc)** - WebSocket implementation

- Zero-copy WebSocket protocol implementation
- Supports per-message deflate compression
- Optimized for performance-critical applications

### Async Design: `impl Future` vs `async`

**Why use `impl Future<Output = Result<...>> + Send + 'static` instead of `async`?**

The Rust compiler generates complete [state machines](https://jeffmcbride.net/blog/2025/05/16/rust-async-functions-as-state-machines/) from the `async` keyword, but there's an important caveat:

When a function captures `&self`, the compiler prevents spawning it with `tokio::spawn`.
This is due to futures not executing until `.await` is called.
The compiler can't guarantee the `&self` object will live for `'static`.
Thus, using `impl Future<...>` explicitly tells the compiler the returned future is `Send` and `'static`.

**Practical Benefits:**

```rust
// Direct spawning (fire and forget)
tokio::spawn(client.place());

// Or deferred spawning
let future = client.place();
tokio::spawn(async move {
    let res = future.await;
    match res {
        ...
    }
})
```

**See for yourself:**

- [Without `impl Future`](https://play.rust-lang.org/?version=stable&mode=debug&edition=2024&gist=fa6d79a7aaca5d63f53e409de375708c) - Doesn't compile with `tokio::spawn`
- [With `impl Future`](https://play.rust-lang.org/?version=stable&mode=debug&edition=2024&gist=d2e1fc1733b8e9c4a0490e8563678b2c) - Compiles and works correctly

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
hypersdk = "0.1"
```

## Quick Start

### HyperCore - Query Markets

```rust
use hypersdk::hypercore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a mainnet client
    let client = hypercore::mainnet();

    // Get perpetual markets
    let perps = client.perps().await?;
    for market in perps {
        println!("{}: {}x leverage", market.name, market.max_leverage);
    }

    // Get spot markets
    let spots = client.spot().await?;
    for market in spots {
        println!("{}", market.symbol());
    }

    Ok(())
}
```

Run it with:

```bash
cargo new --bin my_hl_project
cd my_hl_project
cargo add hypersdk
cargo add anyhow
cargo add tokio --features full
cargo run
```

### HyperCore - Place an Order

```rust
use hypersdk::hypercore::{self, types::*, PrivateKeySigner};
use rust_decimal::{dec, Decimal};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = hypercore::mainnet();
    // You can also use existing Foundry keystores!!
    // let signer = LocalSigner::decrypt_keystore("/home/user/.foundry/keystores/my_user", "123")?;
    let signer: PrivateKeySigner = "your_private_key".parse()?;

    let order = BatchOrder {
        orders: vec![OrderRequest {
            asset: 0, // BTC
            is_buy: true,
            limit_px: dec!(50000),
            sz: dec!(0.1),
            reduce_only: false,
            order_type: OrderTypePlacement::Limit {
                tif: TimeInForce::Gtc,
            },
            cloid: Default::default(),
        }],
        grouping: OrderGrouping::Na,
    };

    let nonce = chrono::Utc::now().timestamp_millis() as u64;
    let result = client.place(&signer, order, nonce, None, None).await?;

    println!("Order placed: {:?}", result);
    Ok(())
}
```

### HyperCore - WebSocket Subscriptions

```rust
use hypersdk::hypercore::{self, types::*};
use futures::StreamExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut ws = hypercore::mainnet_ws();

    // Subscribe to market data
    ws.subscribe(Subscription::Trades { coin: "BTC".into() });
    ws.subscribe(Subscription::L2Book { coin: "ETH".into() });

    // Process incoming messages
    while let Some(msg) = ws.next().await {
        match msg {
            Incoming::Trades(trades) => {
                for trade in trades {
                    println!("{} @ {} size {}", trade.side, trade.px, trade.sz);
                }
            }
            Incoming::L2Book(book) => {
                println!("Order book update for {}", book.coin);
            }
            _ => {}
        }
    }

    Ok(())
}
```

### HyperEVM - Morpho Lending

```rust
use hypersdk::hyperevm::morpho;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = morpho::Client::mainnet().await?;

    // Get highest APY vault
    let vaults = client.highest_apy_vaults(10).await?;
    for vault in vaults {
        println!("{}: {:.2}% APY", vault.name, vault.apy * 100.0);
    }

    // Get specific market APY
    let apy = client.apy(morpho_address, market_id).await?;
    println!("Borrow APY: {:.2}%", apy.borrow * 100.0);
    println!("Supply APY: {:.2}%", apy.supply * 100.0);

    Ok(())
}
```

### HyperEVM - Uniswap V3

```rust
use hypersdk::hyperevm::uniswap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let contracts = uniswap::Contracts::mainnet();
    let client = uniswap::Client::mainnet(contracts).await?;
    let user_address = "0x...".parse().unwrap();

    // Get pool price
    let price = client.get_pool_price(token0, token1, 3000).await?;
    println!("Pool price: {}", price);

    // Get user positions
    let positions = client.positions(user_address).await?;
    for pos in positions {
        println!("Position #{}: {} liquidity", pos.token_id, pos.liquidity);
    }

    Ok(())
}
```

## Examples

There are examples in the `examples/` folder. We tried to cover as many cases as possible.

## Features

- [Price Tick Rounding](#price-tick-rounding)
- [Transfers Support](#transfers-support)
- [HIP-3: Multi-DEX Support](#hip-3-multi-dex-support)
- [Multi-Sig Support](#multi-sig-support)
- [Signature Recovery](#signature-recovery)

### Price Tick Rounding

The SDK includes accurate price tick size calculation for both spot and perpetual markets:

```rust
use hypersdk::hypercore;
use rust_decimal_macros::dec;

let client = hypercore::mainnet();
let perps = client.perps().await?;

// Get BTC market and round a price
let btc = perps.iter().find(|m| m.name == "BTC").unwrap();

// Round to valid tick size
let rounded = btc.round_price(dec!(93231.23)); // Returns 93231

// Directional rounding for order placement
let conservative_ask = btc.round_by_side(Side::Ask, dec!(93231.4), true);  // Rounds up to 93232
let aggressive_bid = btc.round_by_side(Side::Bid, dec!(93231.4), false);   // Rounds up to 93232
```

### Transfers support

Transfer assets between three contexts: perpetual balance, spot balance, and HyperEVM.

```rust
use hypersdk::hypercore::{self, PrivateKeySigner};
use rust_decimal_macros::dec;

let client = hypercore::mainnet();
let signer: PrivateKeySigner = "your_private_key".parse()?;

// Transfer between Core and EVM
client.transfer_to_evm(&signer, dec!(100.0), "USDC", nonce).await?;
client.transfer_from_evm(&signer, dec!(100.0), "USDC", nonce).await?;

// Transfer between perps and spot on Core
client.transfer_to_perps(&signer, dec!(100.0), "USDC", nonce).await?;
client.transfer_to_spot(&signer, dec!(100.0), "USDC", nonce).await?;
```

### HIP-3: Multi-DEX Support

The SDK supports [HIP-3](https://hyperliquid.gitbook.io/hyperliquid-docs/hyperliquid-improvement-proposals-hips/hip-3-builder-deployed-perpetuals),
allowing you to query and trade HIP-3 perpetual markets:

```rust
use hypersdk::hypercore;

let client = hypercore::mainnet();

// Query all available DEXes
let dexes = client.perp_dexs().await?;
for dex in &dexes {
    println!("DEX: {}", dex.name());
}

// Get markets from a specific DEX
if let Some(dex) = dexes.first() {
    let markets = client.perps_from(dex.clone()).await?;
    for market in markets {
        println!("{}: {}x leverage", market.name, market.max_leverage);
    }
}
```

### Multi-Sig Support

The SDK supports multi-signature operations for orders and transfers:

```rust
use hypersdk::hypercore::{self, PrivateKeySigner};

let client = hypercore::mainnet();
let lead_signer: PrivateKeySigner = "lead_key".parse()?;
let signer1: PrivateKeySigner = "key1".parse()?;
let signer2: PrivateKeySigner = "key2".parse()?;
let multisig_address = "0x...".parse()?;
let nonce = chrono::Utc::now().timestamp_millis() as u64;

// Create a multi-sig order
let result = client
    .multi_sig(&lead_signer, multisig_address, nonce)
    .signer(&signer1)
    .signer(&signer2)
    .place(order, None, None)
    .await?;

// Multi-sig transfers
use hypersdk::hypercore::types::UsdSend;

let send = UsdSend {
    destination: "0x0...".parse()?,
    amount: dec!(100.0),
    time: nonce,
};

client
    .multi_sig(&lead_signer, multisig_address, nonce)
    .signers(vec![&signer1, &signer2])
    .send_usdc(send)
    .await?;

// Append pre-existing signatures (useful for offline signature collection)
use hypersdk::hypercore::types::Signature;

let existing_sigs: Vec<Signature> = vec![
    "0xaabbcc...".parse()?,
    "0xddeeff...".parse()?,
];

client
    .multi_sig(&lead_signer, multisig_address, nonce)
    .signatures(existing_sigs)  // Add pre-collected signatures
    .signer(&signer1)            // Can still add more signers
    .place(order, None, None)
    .await?;
```

### Signature Recovery

Recover the signer's address from any signed action:

```rust
use hypersdk::hypercore::{self, types::*, PrivateKeySigner, Chain};

let signer: PrivateKeySigner = "your_private_key".parse()?;
let nonce = chrono::Utc::now().timestamp_millis() as u64;

// Sign an action
let order = BatchOrder { /* ... */ };
let action = Action::Order(order.clone());
let signed = action.sign_sync(&signer, nonce, None, None, Chain::Mainnet)?;

// Recover the address
let recovered = Action::Order(order).recover(
    &signed.signature,
    nonce,
    None,
    None,
    Chain::Mainnet
)?;

assert_eq!(recovered, signer.address());
```

## Configuration

Most examples require a private key set via environment variable:

```bash
export PRIVATE_KEY="your_private_key_here"
```

For development, you can use a `.env` file:

```bash
PRIVATE_KEY=your_private_key_here
```

## Documentation

- [API Documentation](https://docs.rs/hypersdk)
- [Hyperliquid Documentation](https://hyperliquid.gitbook.io/hyperliquid-docs/)
- [Examples](./examples/)

## Development

### Running Tests

```bash
# Run only unit tests
cargo test --lib
```

### Building Documentation

```bash
# Build and open documentation locally
cargo doc --open --no-deps
```

## Requirements

- Rust 1.85.0 or higher
- Tokio async runtime

## License

This project is licensed under the Mozilla Public License 2.0 - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Disclaimer

This software is provided "as is", without warranty of any kind. Use at your own risk. Trading cryptocurrencies involves substantial risk of loss.

## Support

- GitHub Issues: [Report bugs or request features](https://github.com/infinitefield/hypersdk/issues)
- Documentation: [docs.rs/hypersdk](https://docs.rs/hypersdk)

## About us

Infinite Field is a high-frequency trading firm. We build ultra-low-latency systems for execution at scale. Performance is everything.

We prioritize practical solutions over theory. If something works and delivers results, that’s what matters. Performance is always the goal, and every piece of code is written with efficiency and longevity in mind.

If you specialize in performance-critical software, understand systems down to the bare metal, and know how to optimize x64 assembly, we’d love to hear from you.

[Explore career opportunities](https://jobs.ashbyhq.com/infinitefield/)

---

**Note**: This SDK is not officially affiliated with Hyperliquid. It is a community-maintained project.
