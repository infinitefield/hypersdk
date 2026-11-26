# hypersdk

Rust SDK for the [Hyperliquid](https://app.hyperliquid.xyz) protocol, covering HyperCore L1 (HTTP and WebSocket) and HyperEVM contracts.

[![Crates.io](https://img.shields.io/crates/v/hypersdk.svg)](https://crates.io/crates/hypersdk)
[![Documentation](https://docs.rs/hypersdk/badge.svg)](https://docs.rs/hypersdk)
[![License: MPL 2.0](https://img.shields.io/badge/License-MPL_2.0-blue.svg)](https://opensource.org/licenses/MPL-2.0)

> **CLI and AI Agents**: For terminal use or agent workflows without writing Rust, install [`hypecli`](hypecli/README.md) and see [`skills/`](skills/).

## Contents

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Core Concepts](#core-concepts)
- [HyperCore Examples](#hypercore-examples)
  - [Account Balances, Positions, and Open Orders](#1-account-balances-positions-and-open-orders)
  - [Order Lifecycle: Place, Modify, and Cancel](#2-order-lifecycle-place-modify-and-cancel)
  - [Market Orders and Closing Positions](#3-market-orders-and-closing-positions)
  - [WebSocket Subscriptions](#4-websocket-subscriptions)
  - [Splitting a WebSocket and Posting Over the Socket](#5-splitting-a-websocket-and-posting-over-the-socket)
  - [Transfers and HyperEVM Bridging](#6-transfers-and-hyperevm-bridging)
  - [HIP-3 Multi-DEX Perpetuals](#7-hip-3-multi-dex-perpetuals)
  - [API Agents, Vaults, and Multi-Sig](#8-api-agents-vaults-and-multi-sig)
  - [Offline Signing and Signature Recovery](#9-offline-signing-and-signature-recovery)
- [HyperEVM Examples](#hyperevm-examples)
  - [Provider and ERC-20 Queries](#1-provider-and-erc-20-queries)
  - [Morpho Markets and MetaMorpho Vaults](#2-morpho-markets-and-metamorpho-vaults)
  - [Uniswap V3 and PRJX](#3-uniswap-v3-and-prjx)
- [Running Repository Examples](#running-repository-examples)
- [CLI (`hypecli`)](#cli-hypecli)
- [Design Notes](#design-notes)
- [Development](#development)

## Installation

Requires **Rust 1.94.1+** (Edition 2024) and Tokio.

```toml
[dependencies]
hypersdk = "0.3"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
anyhow = "1"
rust_decimal = "1"
futures = "0.3" # needed when consuming WebSocket streams
```

### Cargo Features

| Feature | Default | Enables |
| --- | --- | --- |
| `http` | Yes | `hypercore::HttpClient` (`mainnet()`, `testnet()`) |
| `ws` | Yes | `hypercore::WebSocket` (`mainnet_ws()`, `testnet_ws()`) |
| `hyperevm` | No | `hyperevm` RPC providers and `ERC20` / `IERC4626` bindings |
| `morpho` | No | `hyperevm::morpho` lending markets and MetaMorpho vaults (implies `hyperevm`) |
| `uniswap` | No | `hyperevm::uniswap` V3 pools, positions, and PRJX preset (implies `hyperevm`) |

HyperCore types and EIP-712 / msgpack signing are always available. Disable default features if you only need offline signing and types:

```toml
# Offline types and signing only
hypersdk = { version = "0.3", default-features = false }

# Full L1 + HyperEVM DeFi support
hypersdk = { version = "0.3", features = ["morpho", "uniswap"] }
```

To load encrypted Foundry keystores (`PrivateKeySigner::decrypt_keystore`), add `alloy = { version = "2", default-features = false, features = ["signer-keystore"] }`. Raw hex private keys work out of the box.

## Quick Start

Create a new binary project and query live HyperCore market metadata, mid prices, and top-of-book quotes (no wallet or API key needed):

```bash
cargo new my_hl_app && cd my_hl_app
cargo add hypersdk anyhow rust_decimal
cargo add tokio --features macros,rt-multi-thread
```

Put this in `src/main.rs` and run `cargo run`:

```rust
use hypersdk::hypercore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = hypercore::mainnet();

    // Perpetual and spot market metadata
    let perps = client.perps().await?;
    let spots = client.spot().await?;
    println!("Loaded {} perp markets and {} spot markets", perps.len(), spots.len());

    if let Some(btc) = perps.iter().find(|m| m.name == "BTC") {
        println!(
            "BTC: asset_index={}, max_leverage={}x, sz_decimals={}",
            btc.index, btc.max_leverage, btc.sz_decimals
        );
    }

    // Mid prices across all main-DEX markets
    let mids = client.all_mids(None).await?;
    println!("BTC mid: {:?}", mids.get("BTC"));

    // L2 order book snapshot
    let book = client.l2_book("BTC".to_string(), None, None).await?;
    if let (Some(bid), Some(ask), Some(spread)) = (book.best_bid(), book.best_ask(), book.spread()) {
        println!("BTC book: bid={} ({}) | ask={} ({}) | spread={}", bid.px, bid.sz, ask.px, ask.sz, spread);
    }

    Ok(())
}
```

## Core Concepts

### Networks and Clients

```rust
use std::sync::Arc;
use hypersdk::hypercore::{self, Chain, HttpClient};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Convenience constructors
    let mainnet = hypercore::mainnet();       // https://api.hyperliquid.xyz
    let testnet = hypercore::testnet();       // https://api.hyperliquid-testnet.xyz
    let mut ws = mainnet.websocket();         // wss://api.hyperliquid.xyz/ws

    // Or configure explicitly (chain sets the EIP-712 signing domain)
    let custom = Arc::new(
        HttpClient::new(Chain::Mainnet).with_url("https://api.hyperliquid.xyz".parse()?),
    );

    println!("Client chain: {}", custom.chain());
    let _ = (testnet, &mut ws);
    Ok(())
}
```

`HttpClient` is not `Clone`; wrap it in `Arc<HttpClient>` when sharing across tasks. Creating a `WebSocket` spawns a background connection task, so call `mainnet_ws()`, `testnet_ws()`, or `client.websocket()` inside a Tokio runtime.

### Signers and Nonces

Every signed action takes a millisecond-based `nonce: u64`. Share one thread-safe `NonceHandler` per signing key and call `.next()` once per request. For `UsdSend`, `SpotSend`, and `SendAsset`, pass the same nonce value inside the struct (`time` or `nonce`) and as the method's `nonce` argument.

### Asset Index vs. Coin Name

Order and cancel payloads identify markets by numeric `asset` index, while `/info` queries and WebSocket subscriptions use coin strings. Look markets up at startup rather than hardcoding indices:

| Market Type | Order / Cancel `asset` | Info / WebSocket `coin` |
| --- | --- | --- |
| Main-DEX perp | `perp.index` (`0`, `1`, ...) | `perp.name` (`"BTC"`, `"ETH"`) |
| HIP-3 perp | `perp.index` (`100_000 + dex * 10_000 + i`) | `perp.name` (`"xyz:XYZ100"`) |
| Spot pair | `spot.index` (`10_000 + spot_index`) | `spot.name` (`"PURR/USDC"`, `"@107"`) |
| HIP-4 outcome | `outcome.market` (`100_000_000 + ...`) | `outcome.coin()` (`"#42"`) |

### Decimals, Price Ticks, and Lot Sizes

All prices and sizes use `rust_decimal::Decimal` (re-exported as `hypersdk::{Decimal, dec}`). Orders off tick are rejected by the exchange:

- `market.round_price(px)` rounds to the nearest valid tick (5 significant figures, capped at `6 - sz_decimals` decimals on perps and `8 - sz_decimals` on spot).
- `market.round_by_side(side, px, conservative)` rounds directionally: `conservative = true` rounds bids down and asks up (safer for makers); `conservative = false` rounds bids up and asks down (more likely to fill).
- Truncate order sizes to `perp.sz_decimals` (or `spot.base().sz_decimals`) using `RoundingStrategy::ToZero`.

---

## HyperCore Examples

### 1. Account Balances, Positions, and Open Orders

Read spot balances, perpetual margin summary, open positions, and resting orders for any address. If you trade through an approved API agent, pass the **main account** address here, not the agent's address.

```rust
use hypersdk::{Address, hypercore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = hypercore::mainnet();
    let user: Address = std::env::var("USER_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string())
        .parse()?;

    // Spot token balances
    for bal in client.user_balances(user).await? {
        println!("Spot {}: total={} available={}", bal.coin, bal.total, bal.available());
    }

    // Perpetual margin and positions (pass Some("dex_name".into()) for HIP-3 DEXes)
    let state = client.clearinghouse_state(user, None).await?;
    println!(
        "Perp account_value={} withdrawable={} margin_used={}%",
        state.margin_summary.account_value,
        state.withdrawable,
        state.margin_summary.margin_utilization()
    );

    for item in &state.asset_positions {
        let pos = &item.position;
        println!(
            "Position {} {}: size={} entry={:?} upnl={} liq={:?}",
            pos.side(),
            pos.coin,
            pos.abs_size(),
            pos.entry_px,
            pos.unrealized_pnl,
            pos.liquidation_px
        );
    }

    // Open orders
    for order in client.open_orders(user, None).await? {
        println!("Order #{}: {} {:?} {} @ {}", order.oid, order.coin, order.side, order.sz, order.limit_px);
    }

    Ok(())
}
```

### 2. Order Lifecycle: Place, Modify, and Cancel

Set leverage, round price and size to market constraints, place a post-only (`TimeInForce::Alo`) limit order, amend its price, and cancel the amended order.

Note two exchange behaviors:
1. `client.place` returns `Ok(Vec<OrderResponseStatus>)` when the HTTP request succeeds, even if an individual order was rejected (`OrderResponseStatus::Error`). Always inspect each status.
2. Modifying a resting order assigns a **new** `oid`; cancel using the `oid` returned by `modify`.

```rust
use hypersdk::hypercore::{
    self, Cloid, NonceHandler, OidOrCloid, PrivateKeySigner,
    types::{
        BatchCancel, BatchModify, BatchOrder, Cancel, Modify, OrderGrouping, OrderRequest,
        OrderResponseStatus, OrderTypePlacement, Side, TimeInForce,
    },
};
use rust_decimal::{RoundingStrategy, dec};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let signer: PrivateKeySigner = std::env::var("PRIVATE_KEY")?.parse()?;
    let client = hypercore::testnet();
    let nonces = NonceHandler::default();

    let perps = client.perps().await?;
    let btc = perps
        .iter()
        .find(|m| m.name == "BTC")
        .ok_or_else(|| anyhow::anyhow!("BTC market not found"))?;

    // Set 5x cross margin on BTC
    client
        .update_leverage(&signer, btc.index, true, 5, nonces.next(), None, None)
        .await?;

    // Round bid conservatively (down to tick) and truncate size to sz_decimals
    let bid_px = btc
        .round_by_side(Side::Bid, dec!(60_000.75), true)
        .ok_or_else(|| anyhow::anyhow!("invalid price"))?;
    let size = dec!(0.001).round_dp_with_strategy(btc.sz_decimals as u32, RoundingStrategy::ToZero);

    // 1. Place a post-only limit buy
    let place_statuses = client
        .place(
            &signer,
            BatchOrder {
                orders: vec![OrderRequest {
                    asset: btc.index,
                    is_buy: true,
                    limit_px: bid_px,
                    sz: size,
                    reduce_only: false,
                    order_type: OrderTypePlacement::Limit { tif: TimeInForce::Alo },
                    cloid: Cloid::random(),
                }],
                grouping: OrderGrouping::Na,
                builder: None,
            },
            nonces.next(),
            None,
            None,
        )
        .await?;

    let oid = match &place_statuses[0] {
        OrderResponseStatus::Resting { oid, .. } => *oid,
        OrderResponseStatus::Filled { oid, total_sz, avg_px } => {
            println!("Filled immediately #{oid}: {total_sz} @ {avg_px}");
            return Ok(());
        }
        OrderResponseStatus::Error(reason) => anyhow::bail!("Order rejected: {reason}"),
        other => anyhow::bail!("Unexpected status: {other:?}"),
    };
    println!("Placed resting order #{oid} at {bid_px}");

    // 2. Modify the resting order to a new tick-aligned price
    let new_px = btc
        .round_by_side(Side::Bid, dec!(60_010.25), true)
        .ok_or_else(|| anyhow::anyhow!("invalid price"))?;

    let modify_statuses = client
        .modify(
            &signer,
            BatchModify {
                modifies: vec![Modify {
                    oid: OidOrCloid::Left(oid),
                    order: OrderRequest {
                        asset: btc.index,
                        is_buy: true,
                        limit_px: new_px,
                        sz: size,
                        reduce_only: false,
                        order_type: OrderTypePlacement::Limit { tif: TimeInForce::Alo },
                        cloid: Cloid::random(),
                    },
                }],
                always_place: false,
            },
            nonces.next(),
            None,
            None,
        )
        .await?;

    let amended_oid = match &modify_statuses[0] {
        OrderResponseStatus::Resting { oid, .. } => *oid,
        other => anyhow::bail!("Modify did not rest: {other:?}"),
    };
    println!("Amended order to #{amended_oid} at {new_px}");

    // 3. Cancel the amended order
    let cancel_statuses = client
        .cancel(
            &signer,
            BatchCancel {
                cancels: vec![Cancel { asset: btc.index, oid: amended_oid }],
                fast: false,
            },
            nonces.next(),
            None,
            None,
        )
        .await?;
    println!("Cancel status: {:?}", cancel_statuses[0]);

    Ok(())
}
```

### 3. Market Orders and Closing Positions

`market_open` submits an immediate-or-cancel market order up to a tick-rounded worst acceptable price (`PerpMarket`, `SpotMarket`, and `OutcomeMarket` all implement `Market`). To close an existing position without flipping direction, submit an IOC order on the opposite side with `reduce_only: true`.

```rust
use hypersdk::hypercore::{
    self, Cloid, NonceHandler, PrivateKeySigner,
    types::{
        BatchOrder, OrderGrouping, OrderRequest, OrderResponseStatus, OrderTypePlacement, Side,
        TimeInForce,
    },
};
use rust_decimal::dec;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let signer: PrivateKeySigner = std::env::var("PRIVATE_KEY")?.parse()?;
    let client = hypercore::testnet();
    let nonces = NonceHandler::default();

    let perps = client.perps().await?;
    let eth = perps
        .iter()
        .find(|m| m.name == "ETH")
        .ok_or_else(|| anyhow::anyhow!("ETH market not found"))?;

    let mids = client.all_mids(None).await?;
    let mid = *mids.get("ETH").ok_or_else(|| anyhow::anyhow!("no ETH mid"))?;

    // Market buy 0.01 ETH with 1% max slippage, rounded aggressively (up for a buy)
    let max_buy_px = eth
        .round_by_side(Side::Bid, mid * dec!(1.01), false)
        .ok_or_else(|| anyhow::anyhow!("invalid price"))?;

    let open_statuses = client
        .market_open(&signer, eth, true, max_buy_px, dec!(0.01), nonces.next(), None, None, None)
        .await?;

    if let OrderResponseStatus::Filled { total_sz, avg_px, oid } = &open_statuses[0] {
        println!("Opened #{oid}: bought {total_sz} ETH @ {avg_px}");

        // Close the position with a reduce-only IOC sell (1% max slippage down)
        let min_sell_px = eth
            .round_by_side(Side::Ask, mid * dec!(0.99), false)
            .ok_or_else(|| anyhow::anyhow!("invalid price"))?;

        let close_statuses = client
            .place(
                &signer,
                BatchOrder {
                    orders: vec![OrderRequest {
                        asset: eth.index,
                        is_buy: false,
                        limit_px: min_sell_px,
                        sz: *total_sz,
                        reduce_only: true,
                        order_type: OrderTypePlacement::Limit { tif: TimeInForce::Ioc },
                        cloid: Cloid::random(),
                    }],
                    grouping: OrderGrouping::Na,
                    builder: None,
                },
                nonces.next(),
                None,
                None,
            )
            .await?;
        println!("Close status: {:?}", close_statuses[0]);
    } else {
        println!("Open status: {:?}", open_statuses[0]);
    }

    Ok(())
}
```

### 4. WebSocket Subscriptions

`WebSocket` reconnects automatically with backoff and replays active subscriptions on reconnect. `subscribe` queues the subscription synchronously; watch for `Incoming::Error` in case a subscription is rejected, and skip `is_snapshot == true` on `UserFills` if you only want live fills.

```rust
use futures::StreamExt;
use hypersdk::{
    Address,
    hypercore::{
        self,
        types::{Incoming, Subscription},
        ws::Event,
    },
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut ws = hypercore::mainnet_ws();

    ws.subscribe(Subscription::Bbo { coin: "BTC".into() });
    ws.subscribe(Subscription::Trades { coin: "BTC".into() });
    ws.subscribe(Subscription::Candle {
        coin: "BTC".into(),
        interval: "1m".into(),
    });

    if let Ok(addr) = std::env::var("USER_ADDRESS") {
        let user: Address = addr.parse()?;
        ws.subscribe(Subscription::OrderUpdates { user });
        ws.subscribe(Subscription::UserFills { user });
    }

    while let Some(event) = ws.next().await {
        match event {
            Event::Connected => println!("WebSocket connected"),
            Event::Disconnected => println!("WebSocket disconnected, reconnecting"),
            Event::Message(msg) => match msg {
                Incoming::Bbo(bbo) => {
                    if let (Some(mid), Some(spread)) = (bbo.mid(), bbo.spread()) {
                        println!("{} mid={mid} spread={spread}", bbo.coin);
                    }
                }
                Incoming::Trades(trades) => {
                    for t in trades {
                        println!("trade {} {:?} {} @ {}", t.coin, t.side, t.sz, t.px);
                    }
                }
                Incoming::Candle(c) => {
                    println!("candle {} {} O={} H={} L={} C={}", c.coin, c.interval, c.open, c.high, c.low, c.close);
                }
                Incoming::OrderUpdates(updates) => {
                    for u in updates {
                        println!("order #{} -> {}", u.order.oid, u.status);
                    }
                }
                Incoming::UserFills { is_snapshot, fills, .. } => {
                    if is_snapshot {
                        continue; // initial fill history sent on subscribe
                    }
                    for f in fills {
                        println!("live fill {} {} @ {}", f.coin, f.sz, f.px);
                    }
                }
                Incoming::Error(err) => eprintln!("Subscription error: {err}"),
                _ => {}
            },
        }
    }

    Ok(())
}
```

### 5. Splitting a WebSocket and Posting Over the Socket

Requires `serde_json = "1"` in `Cargo.toml`. Use `ws.split()` to separate the cloneable `ConnectionHandle` (for subscribing or sending requests from other tasks) from the `ConnectionStream`, and `handle.post(id, ...)` to send `/info` or signed `/exchange` requests over the open socket without HTTP round-trip setup. Keep at least one `ConnectionHandle` alive while reading `ConnectionStream`.

```rust
use futures::StreamExt;
use hypersdk::hypercore::{
    self,
    types::{Incoming, PostRequest, PostResponsePayload},
    ws::Event,
};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (handle, mut stream) = hypercore::mainnet_ws().split();
    let sender = handle.clone();

    // Send info requests from another task using distinct request IDs
    tokio::spawn(async move {
        sender.post(1, PostRequest::Info(json!({ "type": "l2Book", "coin": "BTC" })));
        sender.post(2, PostRequest::Info(json!({ "type": "allMids" })));
    });

    let mut received = 0;
    while let Some(event) = stream.next().await {
        let Event::Message(Incoming::Post(reply)) = event else {
            continue;
        };

        match reply.response {
            PostResponsePayload::Info(value) => {
                println!("Reply #{}: type={}", reply.id, value["type"]);
            }
            PostResponsePayload::Action(resp) => {
                println!("Action reply #{}: {resp:?}", reply.id);
            }
            PostResponsePayload::Error(err) => {
                eprintln!("Post #{} failed: {err}", reply.id);
            }
        }

        received += 1;
        if received == 2 {
            break;
        }
    }

    drop(handle);
    Ok(())
}
```

### 6. Transfers and HyperEVM Bridging

Move funds between perpetual balances, spot balances, external addresses, and HyperEVM:

- `send_usdc`: perp USDC to another address's perp balance (`UsdSend.time` must equal `nonce`).
- `transfer_to_perps` / `transfer_to_spot`: USDC between your own spot and perp balances (takes the `USDC` `SpotToken`).
- `spot_send`: any spot token to another address (`SpotSend.time` must equal `nonce`).
- `transfer_to_evm`: bridges a `SpotToken` from HyperCore spot to your HyperEVM wallet. To send tokens back from HyperEVM to HyperCore, send an EVM transaction to `token.cross_chain_address` using `token.to_wei(amount)` (see [`examples/hypercore/transfer_from_evm.rs`](examples/hypercore/transfer_from_evm.rs)).

```rust
use hypersdk::{
    Address,
    hypercore::{
        self, NonceHandler, PrivateKeySigner,
        types::{SendToken, SpotSend, UsdSend},
    },
};
use rust_decimal::dec;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let signer: PrivateKeySigner = std::env::var("PRIVATE_KEY")?.parse()?;
    let recipient: Address = std::env::var("RECIPIENT")?.parse()?;
    let client = hypercore::testnet();
    let nonces = NonceHandler::default();

    // 1. Send perp USDC to another address
    let nonce = nonces.next();
    client
        .send_usdc(
            &signer,
            UsdSend { destination: recipient, amount: dec!(10), time: nonce },
            nonce,
        )
        .await?;

    // Look up spot tokens once for spot / perp / EVM transfers
    let tokens = client.spot_tokens().await?;
    let usdc = tokens
        .iter()
        .find(|t| t.name == "USDC")
        .ok_or_else(|| anyhow::anyhow!("USDC token not found"))?
        .clone();

    // 2. Move USDC between your own spot and perpetual balances
    client.transfer_to_perps(&signer, usdc.clone(), dec!(25), nonces.next()).await?;
    client.transfer_to_spot(&signer, usdc.clone(), dec!(25), nonces.next()).await?;

    // 3. Send a spot token to another address
    let nonce = nonces.next();
    client
        .spot_send(
            &signer,
            SpotSend {
                destination: recipient,
                token: SendToken(usdc),
                amount: dec!(5),
                time: nonce,
            },
            nonce,
        )
        .await?;

    // 4. Bridge a linked spot token from HyperCore to your HyperEVM address
    if let Some(hype) = tokens.iter().find(|t| t.name == "HYPE" && t.cross_chain_address.is_some()) {
        client
            .transfer_to_evm(&signer, hype.clone(), dec!(0.1), nonces.next())
            .await?;
    }

    Ok(())
}
```

### 7. HIP-3 Multi-DEX Perpetuals

Query builder-deployed [HIP-3](https://hyperliquid.gitbook.io/hyperliquid-docs/hyperliquid-improvement-proposals-hips/hip-3-builder-deployed-perpetuals) perpetual DEXes, their markets, and deployer settings:

```rust
use hypersdk::hypercore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = hypercore::mainnet();

    let dexes = client.perp_dexes().await?;
    for dex in &dexes {
        println!("DEX: {} (index {})", dex.name(), dex.index());
    }

    if let Some(first_dex) = dexes.first().cloned() {
        let markets = client.perps_from(first_dex).await?;
        for market in markets.iter().take(5) {
            println!("{}: asset={} max_leverage={}x", market.name, market.index, market.max_leverage);
        }
    }

    // Full DEX configuration (deployer, oracle signers, sub-deployers)
    for details in client.perp_dex_details().await? {
        println!(
            "{}: deployer={:?} oracle_signers={:?}",
            details.name,
            details.deployer,
            details.sub_deployers_for("setOracle")
        );
    }

    Ok(())
}
```

### 8. API Agents, Vaults, and Multi-Sig

#### Approve an API Agent and Interact with Vaults

```rust
use hypersdk::{
    Address,
    hypercore::{self, NonceHandler, PrivateKeySigner},
};
use rust_decimal::dec;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let owner: PrivateKeySigner = std::env::var("PRIVATE_KEY")?.parse()?;
    let vault: Address = std::env::var("VAULT_ADDRESS")?.parse()?;
    let client = hypercore::testnet();
    let nonces = NonceHandler::default();

    // Generate and approve a dedicated API wallet for bot order signing
    let agent = PrivateKeySigner::random();
    client
        .approve_agent(&owner, agent.address(), "bot".to_string(), nonces.next())
        .await?;
    println!("Approved agent {}", agent.address());

    // Inspect vault performance and deposit 100 USDC
    let details = client.vault_details(vault, Some(owner.address())).await?;
    println!("Vault {} APR: {}", details.name, details.apr);

    client
        .vault_transfer(&owner, vault, dec!(100), nonces.next(), true)
        .await?;

    Ok(())
}
```

#### Multi-Signature Orders and Transfers

`client.multi_sig(&lead_signer, multisig_address, nonce)` builds a multi-sig request across signers (and/or pre-collected offline `Signature`s). For `send_usdc` and `send_asset`, `UsdSend.time` / `SendAsset.nonce` must match the `nonce` passed to `multi_sig`.

```rust
use hypersdk::{
    Address,
    hypercore::{
        self, Cloid, NonceHandler, PrivateKeySigner,
        types::{
            BatchOrder, OrderGrouping, OrderRequest, OrderTypePlacement, Signature, TimeInForce,
            UsdSend,
        },
    },
};
use rust_decimal::dec;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let lead: PrivateKeySigner = std::env::var("LEAD_KEY")?.parse()?;
    let signer2: PrivateKeySigner = std::env::var("SIGNER2_KEY")?.parse()?;
    let multisig_addr: Address = std::env::var("MULTISIG_ADDRESS")?.parse()?;
    let recipient: Address = std::env::var("RECIPIENT")?.parse()?;

    let client = hypercore::testnet();
    let nonces = NonceHandler::default();

    let perps = client.perps().await?;
    let btc = perps
        .iter()
        .find(|m| m.name == "BTC")
        .ok_or_else(|| anyhow::anyhow!("BTC not found"))?;

    let order = BatchOrder {
        orders: vec![OrderRequest {
            asset: btc.index,
            is_buy: true,
            limit_px: dec!(60_000),
            sz: dec!(0.001),
            reduce_only: false,
            order_type: OrderTypePlacement::Limit { tif: TimeInForce::Alo },
            cloid: Cloid::random(),
        }],
        grouping: OrderGrouping::Na,
        builder: None,
    };

    // Multi-sig order signed by lead + signer2
    let statuses = client
        .multi_sig(&lead, multisig_addr, nonces.next())
        .signers([&lead, &signer2])
        .place(order.clone(), None, None)
        .await?;
    println!("Multi-sig order statuses: {statuses:?}");

    // Multi-sig USDC transfer
    let send_nonce = nonces.next();
    client
        .multi_sig(&lead, multisig_addr, send_nonce)
        .signers([&lead, &signer2])
        .send_usdc(UsdSend {
            destination: recipient,
            amount: dec!(50),
            time: send_nonce,
        })
        .await?;

    // You can also attach signatures collected offline
    let offline_sigs: Vec<Signature> = Vec::new();
    let _ = client
        .multi_sig(&lead, multisig_addr, nonces.next())
        .signatures(offline_sigs)
        .signer(&lead);

    Ok(())
}
```

### 9. Offline Signing and Signature Recovery

Sign any `Action` and recover its signer address without making network calls (works with `default-features = false`):

```rust
use hypersdk::hypercore::{
    Chain, Cloid, NonceHandler, PrivateKeySigner,
    types::{Action, BatchOrder, OrderGrouping, OrderRequest, OrderTypePlacement, TimeInForce},
};
use rust_decimal::dec;

fn main() -> anyhow::Result<()> {
    let signer = PrivateKeySigner::random();
    let nonce = NonceHandler::default().next();

    let batch = BatchOrder {
        orders: vec![OrderRequest {
            asset: 0,
            is_buy: true,
            limit_px: dec!(50_000),
            sz: dec!(0.1),
            reduce_only: false,
            order_type: OrderTypePlacement::Limit { tif: TimeInForce::Gtc },
            cloid: Cloid::random(),
        }],
        grouping: OrderGrouping::Na,
        builder: None,
    };

    let signed = Action::Order(batch.clone()).sign_sync(&signer, nonce, None, None, Chain::Mainnet)?;
    let recovered = Action::Order(batch).recover(&signed.signature, nonce, None, None, Chain::Mainnet)?;

    assert_eq!(recovered, signer.address());
    println!("Verified signature from {}", recovered);
    Ok(())
}
```

---

## HyperEVM Examples

### 1. Provider and ERC-20 Queries

Enable `hyperevm = { ... }` (`hypersdk = { version = "0.3", features = ["hyperevm"] }`):

```rust
use hypersdk::{
    Address,
    hypercore::USDC_CONTRACT_IN_EVM,
    hyperevm::{self, ERC20, ProviderTrait, from_wei},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let provider = hyperevm::mainnet().await?;
    let block = provider.get_block_number().await?;
    println!("HyperEVM block: {block}");

    let usdc = ERC20::new(USDC_CONTRACT_IN_EVM, provider);
    let symbol = usdc.symbol().call().await?;
    let decimals = usdc.decimals().call().await?;

    let holder: Address = "0x0000000000000000000000000000000000000001".parse()?;
    let raw_balance = usdc.balanceOf(holder).call().await?;
    println!("{symbol} balance: {}", from_wei(raw_balance, u32::from(decimals)));

    Ok(())
}
```

### 2. Morpho Markets and MetaMorpho Vaults

Enable `morpho` (`hypersdk = { version = "0.3", features = ["morpho"] }`):

```rust
use hypersdk::{
    Address,
    hyperevm::morpho::{self, MarketId, MetaClient},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Individual Morpho Blue market borrow & supply APY
    let market_client = morpho::Client::mainnet().await?;
    let morpho_contract: Address = std::env::var("MORPHO_ADDRESS")?.parse()?;
    let market_id: MarketId = std::env::var("MARKET_ID")?.parse()?;

    let pool_apy = market_client
        .apy::<f64, _>(morpho_contract, market_id, f64::exp)
        .await?;
    println!(
        "Market borrow={:.2}% supply={:.2}%",
        pool_apy.borrow * 100.0,
        pool_apy.supply * 100.0
    );

    // MetaMorpho vault net APY across allocated markets
    let meta_client = MetaClient::mainnet().await?;
    let vault_addr: Address = std::env::var("VAULT_ADDRESS")?.parse()?;
    let vault_apy = meta_client.apy::<f64, _>(vault_addr, f64::exp).await?;
    let net_apy = vault_apy.apy(|v| v.to::<u128>() as f64) * 100.0;
    println!("Vault net APY ({} markets): {:.2}%", vault_apy.market_count(), net_apy);

    Ok(())
}
```

### 3. Uniswap V3 and PRJX

Enable `uniswap` (`hypersdk = { version = "0.3", features = ["uniswap"] }`). Use `uniswap::prjx::mainnet()` for the built-in PRJX contract deployment on HyperEVM, or pass custom `uniswap::Contracts` to `uniswap::Client::mainnet(contracts)`:

```rust
use hypersdk::{
    Address, address,
    hyperevm::{
        self,
        uniswap::{FEES, prjx},
    },
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = prjx::mainnet().await?;
    let ubtc: Address = address!("0x9fdbda0a5e284c32744d2f17ee5c74b284993463");

    // Pool spot price (0.3% fee tier = 3000)
    let price = client
        .get_pool_price(hyperevm::WHYPE_ADDRESS, ubtc, FEES[2])
        .await?;
    println!("WHYPE/UBTC price: {price}");

    // LP NFT positions and uncollected fees for an address
    if let Ok(lp_env) = std::env::var("LP_ADDRESS") {
        let lp: Address = lp_env.parse()?;
        for pos in client.positions(lp).await? {
            println!(
                "Position #{}: in_range={} provided=({}, {}) fees=({}, {})",
                pos.token_id,
                pos.in_range,
                pos.token0_provided,
                pos.token1_provided,
                pos.token0_fees,
                pos.token1_fees
            );
        }
    }

    Ok(())
}
```

---

## Running Repository Examples

Runnable programs in [`examples/`](examples/) cover read-only queries, trading, WebSockets, multi-sig, and HyperEVM protocols. Set `PRIVATE_KEY` in your environment or a `.env` file for state-changing examples.

```bash
# Read-only HyperCore queries and streams
cargo run --example list-markets
cargo run --example list-tokens
cargo run --example list-hip3
cargo run --example hip3-dex-details
cargo run --example user_balances -- 0x0000000000000000000000000000000000000000
cargo run --example vault_details -- 0xdfc24b077bc1425ad1dea75bcb6f8158e10df303
cargo run --example websocket
cargo run --example websocket-candles
cargo run --example websocket-post

# Trading, transfers, and multi-sig (requires credentials)
cargo run --example send_order -- --help
cargo run --example market_order -- --help
cargo run --example send_usd -- --help
cargo run --example transfer_to_perps -- --help
cargo run --example transfer_to_evm -- --help
cargo run --features hyperevm --example transfer_from_evm -- --help
cargo run --example multisig_order -- --help
cargo run --example error_handling -- --help

# HyperEVM protocols
cargo run --features morpho --example morpho_vault_apy -- --help
cargo run --features morpho --example morpho_supply_apy -- --help
cargo run --features morpho --example morpho_highest_apy -- --rpc-url https://rpc.hyperliquid.xyz/evm
cargo run --features uniswap --example uniswap_pools_created -- --rpc-url https://rpc.hyperliquid.xyz/evm
```

See [`examples/README.md`](examples/README.md) for the complete list and arguments.

## CLI (`hypecli`)

[`hypecli`](hypecli/README.md) is a companion CLI built on `hypersdk` for market queries, Foundry keystores, hardware wallets (Ledger and Trezor), trading, transfers, validator operations, and P2P multi-sig coordination:

```bash
# Install prebuilt binary or build from crates.io
curl -fsSL https://raw.githubusercontent.com/infinitefield/hypersdk/main/hypecli/install.sh | sh
# or: cargo install hypecli

hypecli perps
hypecli spot
hypecli balance 0x0000000000000000000000000000000000000000
hypecli subscribe trades BTC
```

## Design Notes

### Core Dependencies

- **[alloy](https://alloy.rs/)**: EVM contract bindings, primitives (`Address`, `U256`), and EIP-712 / keystore signing.
- **[rust_decimal](https://docs.rs/rust_decimal)**: Exact base-10 fixed-point arithmetic for prices, sizes, and tick rounding.
- **[yawc](https://docs.rs/yawc)**: Zero-copy WebSocket implementation with per-message deflate compression.

### Why `impl Future + Send + 'static` on Hot-Path Actions?

Standard `async fn(&self, ...)` futures capture `&self`, which prevents passing them directly to `tokio::spawn` because the borrow is not `'static` ([without `impl Future`](https://play.rust-lang.org/?version=stable&mode=debug&edition=2024&gist=fa6d79a7aaca5d63f53e409de375708c) vs. [with `impl Future`](https://play.rust-lang.org/?version=stable&mode=debug&edition=2024&gist=d2e1fc1733b8e9c4a0490e8563678b2c)).

Methods such as `place`, `cancel`, `cancel_by_cloid`, `modify`, `spot_send`, and `send_asset` sign eagerly at call time, clone the inner connection handle, and return an owned `Send + 'static` future. You can spawn them without wrapping `HttpClient` or your signer in locks or `Arc`s:

```rust
use hypersdk::hypercore::{
    self, Cloid, NonceHandler, PrivateKeySigner,
    types::{BatchOrder, OrderGrouping, OrderRequest, OrderTypePlacement, TimeInForce},
};
use rust_decimal::dec;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = hypercore::testnet();
    let signer = PrivateKeySigner::random();
    let nonces = NonceHandler::default();

    let batch = BatchOrder {
        orders: vec![OrderRequest {
            asset: 0,
            is_buy: true,
            limit_px: dec!(50_000),
            sz: dec!(0.01),
            reduce_only: false,
            order_type: OrderTypePlacement::Limit { tif: TimeInForce::Alo },
            cloid: Cloid::random(),
        }],
        grouping: OrderGrouping::Na,
        builder: None,
    };

    // Signs synchronously here; `client` and `signer` are not borrowed by `fut`
    let fut = client.place(&signer, batch, nonces.next(), None, None);
    let handle = tokio::spawn(async move {
        if let Err(err) = fut.await {
            eprintln!("Order failed: {} (cloids: {:?})", err.message(), err.ids());
        }
    });

    handle.await?;
    Ok(())
}
```

## Development

```bash
# Run unit tests
cargo test --lib --all-features

# Check all targets and examples across features
cargo check --all-targets --all-features

# Build local API documentation
cargo doc --no-deps --all-features --open
```

## Documentation and Support

- [API Documentation (docs.rs)](https://docs.rs/hypersdk)
- [Hyperliquid API Docs](https://hyperliquid.gitbook.io/hyperliquid-docs/)
- [Examples Guide](examples/README.md)
- [GitHub Issues](https://github.com/infinitefield/hypersdk/issues)

## Contributing

Contributions are welcome! Feel free to open an issue or submit a Pull Request.

## License

Licensed under the Mozilla Public License 2.0. See [LICENSE](LICENSE) for details.

## Disclaimer

This software is provided "as is", without warranty of any kind. Use at your own risk. Trading cryptocurrencies involves substantial risk of loss.

## About Us

Infinite Field is a high-frequency trading firm. We build ultra-low-latency systems for execution at scale. Performance is everything.

We prioritize practical solutions over theory. If something works and delivers results, that's what matters. Performance is always the goal, and every piece of code is written with efficiency and longevity in mind.

If you specialize in performance-critical software, understand systems down to the bare metal, and know how to optimize x64 assembly, we'd love to hear from you.

[Explore career opportunities](https://jobs.ashbyhq.com/infinitefield/)

---

**Note**: This SDK is not officially affiliated with Hyperliquid. It is a community-maintained project.
