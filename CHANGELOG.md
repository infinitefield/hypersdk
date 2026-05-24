# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Added

- `BasicOrder` trigger-order fields from `frontendOpenOrders`: `is_trigger`, `trigger_px`, `trigger_condition`, `is_position_tpsl`
- `OrderResponseStatus::WaitingForTrigger` and `WaitingForFill` order response variants

### Fixed

- Use `userAbstraction` as the info request type for `AbstractionMode` queries

## [v0.2.10]

### Added

- Optional `nSigFigs` and `mantissa` parameters on the `L2Book` WS subscription for price-level aggregation
- Outcome (prediction) market support: `OutcomeMeta`, `OutcomeInfo`, `OutcomeQuestion`, `OutcomeSideSpec` types
- `HttpClient::outcome_meta()` and standalone `hypercore::outcome_meta()` for querying HIP-4 markets
- HyperCore WS subscriptions for `userEvents`, `userTwapSliceFills`, `userTwapHistory`, `activeAssetData`, and `webData2`
- New typed WS payloads for user events, TWAP slice/history streams, and active asset data
- Forward-compatible fallback parsing for unknown `userEvents` payload variants
- New example: `examples/hypercore/websocket-user-events.rs`

### Fixed

- Parse channel "user" fill notifications as `UserEvents` on WebSocket
- Fix `OutcomeMarket::market` to store full asset ID (including 100_000_000 offset)
- Consolidate `AbstractionMode` into `types/api.rs`, remove wrapper type

### Changed

- Extended WebSocket docs/snippets in README and crate docs to include advanced user streams
- Added serde test coverage for the new WS channels and payload schemas

---

## [v0.2.9]

### Added

- Agent-signed send asset (`AgentSendAsset`) action type with L1/RMP signing
- Order write priority support via `BatchOrder.grouping = OrderGrouping::PriorityRate(bps)`

### Changed

- `GossipPriorityBid.max_gas` serialization fixed to use decimal string format
- Dynamic HYPE decimals used for priority bid conversions in CLI and examples

---

## [v0.2.8]

### Added

- Gossip priority auction support: Dutch auction bids for read-priority gossip data
  - `GossipPriorityBid` action type with RMP-based signing
  - `GossipPriorityAuctionStatus` response type and info request
  - `HttpClient::gossip_priority_bid()` and `HttpClient::gossip_priority_auction_status()` methods
  - Example: `examples/hypercore/priority-fee-bid.rs`
  - `hypecli prio bid` command for CLI priority bidding

### Changed

- Improved HTTP error handling and response parsing
- Morpho vault APY: avoid panic on insufficient data; added `get_pool_address` for Uniswap pools
- Support spot asset in order/position listings

---

## [v0.2.7]

### Added

- Vault deposit and withdraw: `HttpClient::vault_transfer()`, `VaultTransfer` action type
- Refactored vault CLI to use subcommands (deposit/withdraw)

### Fixed

- Use max timestamp for TVL in vault details to prevent incorrect calculations

### Changed

- Updated `http.rs` doc comments for improved accuracy

---

## [v0.2.6]

### Added

- `dex` parameter to `WebData2` subscription and incoming types for multi-DEX WebSocket data
- `OrderUpdate` is now generic over the order type for flexible deserialization
- Aligned quote token and deployer fee fields on perpetual markets
- Growth mode field on perpetual markets

### Fixed

- Handle panics when metadata references indices beyond array bounds
- Serialize `OidOrCloid::Right` as hex string for correct action hash computation
- Skip serializing zero cloid in `OrderRequest` to match server hashing behavior

### Changed

- Refactored `PriceTick` to provide public constructors (`for_perp()`, `for_spot()`)

---

## [v0.2.5]

### Added

- `HttpClient::user_fills_by_time()` — query fills within a time range
- Advanced WebSocket user stream subscriptions: `UserEvents`, `UserFills`, `UserTwapSliceFills`,
  `UserTwapHistory`, `ActiveAssetCtx`, `WebData2`
- Tolerate missing TWAP history descriptions gracefully

### Changed

- Filter out non-text WebSocket frames (binary frames are logged as warnings instead of causing errors)

---

## [v0.2.4]

### Added

- `users` field, `taker_address()`, and `maker_address()` on `Trade` struct for identifying trade participants

### Fixed

- CLOID serialization fix for correct action hash matching between client and server

---

## [v0.2.3]

### Added

- Outcome market support: `OutcomeMeta`, `OutcomeInfo`, `OutcomeQuestion`, `OutcomeSideSpec` types
- `HttpClient::outcome_meta()` and standalone `hypercore::outcome_meta()` for querying outcome markets

---

## [v0.2.2]

### Added

- `update_leverage` action and `HttpClient::update_leverage()` client method
- `Noop` action for nonce invalidation
- EVM user modify (`EvmUserModify`) action for toggling big blocks
- Account abstraction mode: `AbstractionMode` query and set via agent-signed and user-signed actions
- Subaccount, vault details, user vault equities, and user role info calls
- `ClearinghouseState` and `funding_history` endpoints
- `UpdateIsolatedMargin` action type with signature recovery support
- `UserRole` enum replacing string-based role responses
- WebSocket connection status events: `Event::Connected`, `Event::Disconnected`, `Event::Message`
- `NonceHandler` concurrency fix for atomic nonce generation
- `NoCross` margin mode added to perpetual universe items
- Reply to server pings on WebSocket; force reconnect after missed pongs

### Documentation

- Added AI agent instructions pointing to skills folder
- Added curl install command to README
- Enhanced trading skill with HIP-3 DEX listing and trading guidance

### Changed

- Unified asset format for subscribe commands in hypecli
- Use `FromStr` for CLOID parsing instead of manual hex decode
- Simplified `hypecli send` command
- Added `--skip-hip3` flag to balance command
- Made user a positional argument in balance command

### Fixed

- Morpho deposits: fix `totalAssets = deposits` calculation
- Fix yawc dependency version

---

## [v0.2.1]

### Added

- Multi-signature transaction support in hypersdk
  - `Action::sign()`, `sign_sync()`, `prehash()`, and `recover()` directly on `Action`
  - Split `types.rs` into modular structure: `types/mod.rs`, `types/api.rs`, `types/solidity.rs`
  - Signature recovery and prehash functionality
  - `multi_sig_config()` and `api_agents()` HTTP client methods

### Changed

- **Breaking**: Removed `Signable` trait; moved signing logic to `Action` enum
- Refactored signing module from 700+ lines to ~26 lines

---

## [v0.2.0]

### Changed

- **Breaking**: Morpho APY calculations now use generic types for high precision arithmetic
  - `PoolApy` and `VaultApy` are generic over `T128` type parameter
  - `apy()` methods require conversion functions
- Added `Cancel` variant to `OkResponse` enum

### Dependencies

- Force rustls as TLS backend across the project
- Updated reqwest to v0.13

## [v0.1.5] - 2026-01-12

### Added

- Added `Cancel` variant to `OkResponse` enum for order cancellation responses
- Added test case for cancel response deserialization
- Added credentials example showing common argument patterns across examples
- Made signing module public (`pub mod signing`)

### Changed

- **Breaking**: Morpho APY calculations now use generic types for high precision arithmetic
  - `PoolApy` and `VaultApy` are now generic over `T128` type parameter
  - `apy()` methods require conversion functions to handle custom numeric types (f64, Decimal, etc.)
  - Enables arbitrary precision calculations for financial computations
- Refactored examples to use common credential and argument handling patterns
- Updated morpho examples to use new generic APY API with explicit conversions

### Fixed

- Fixed API type structs for cancellation and modification responses

### Dependencies

- hypecli v0.1.3: Updated dependencies including alloy and hypersdk versions

**Files Changed**: 24 files, +556 insertions, -249 deletions

---

## [v0.1.4] - 2026-01-10

### Added

- Created new `types/api.rs` module with core API request/response types (788 lines)
- Added `types/solidity.rs` module for Solidity type conversions
- Added signature recovery functionality to `Action` enum (`recover()` method)
- Added `prehash()` method to `Action` for obtaining signing hashes without signing
- Added methods to `HttpClient`: `multi_sig_config()`, `api_agents()`

### Changed

- **Breaking**: Removed `Signable` trait in favor of methods directly on `Action` enum
  - Actions now implement `sign()`, `sign_sync()`, `prehash()`, and `recover()` directly
  - Simplified signing API with unified interface through `Action` enum
- **Breaking**: Split `types.rs` into modular structure: `types/mod.rs`, `types/api.rs`, `types/solidity.rs`
- Reorganized HTTP client to use new type organization
- Refactored signing module, reducing from 700+ lines to ~26 lines by moving logic to `Action`
- hypecli: Switched to simple P2P connections instead of gossip protocol in iroh integration
- hypecli: Enhanced multisig handling with improved error handling and flow

### Documentation

- Added comprehensive module-level documentation for `types/api.rs`
- Improved signing module documentation explaining new architecture
- Updated README with 33 lines of new content

### Dependencies

- hypecli: Upgraded to hypersdk 0.1.3
- hypecli: Force rustls for TLS (added `reqwest-rustls-tls` feature)
- hypecli: Updated dependency versions in Cargo.lock (344 fewer lines after optimization)

**Files Changed**: 14 files, +1477 insertions, -2072 deletions

---

## [v0.1.3] - 2026-01-10

### Changed

- Forced rustls as TLS backend across the project
- Updated reqwest to v0.13
- Updated hypecli README (288 lines reduced, streamlined documentation)
- Refactored multisig module in hypecli (266 lines changed)

### Fixed

- Removed musl target from release workflow
- hypecli: Force connection to endpoint during signing operations

### Dependencies

- Updated various dependency versions in hypecli Cargo.lock

**Files Changed**: 9 files, +290 insertions, -459 deletions

---

## [v0.1.2] - 2026-01-10

### Added

- **hypecli**: New command-line tool for Hyperliquid interactions
  - Added balances, markets, morpho, and multisig modules
  - P2P multisig coordination using iroh-gossip
  - Support for converting accounts to/from multisig
  - User and multisig conversion commands
- **HTTP Client**: New multisig-related functionality
  - `multi_sig_config()` - Query multisig configuration
  - `api_agents()` - Retrieve API agents for a user
- **Signing**: Support for multisig actions and agent approvals
  - `ApproveAgent` action type
  - `ConvertToMultiSigUser` action type
  - Enhanced `MultiSigAction` handling

### Changed

- Exposed additional types and functions in hypercore module for public API
- Updated function signatures for improved API clarity
- Enhanced HTTP client methods with better error handling

### Fixed

- Fixed multisig signing flow for mainnet deployment
- Resolved test failures and compiler warnings

### Dependencies

- Added iroh-gossip, iroh-tickets for P2P coordination
- Added various CLI dependencies (clap, rpassword, indicatif)

**Files Changed**: 20 files, +5632 insertions, -1163 deletions

---

## [v0.1.1] - 2026-01-08

### Added

- **WebSocket Candle Feed**: Real-time candlestick data streaming
  - `candle_snapshot()` - Get historical candle data
  - WebSocket subscription for live candle updates
  - Example: `examples/hypercore/websocket-candles.rs`
- **NonceHandler**: Thread-safe nonce generation utility
  - Atomic timestamp-based nonce generation
  - Prevents replay attacks with monotonic increasing nonces
  - Comprehensive documentation with usage examples
- **Examples**: Added market listing example (`examples/hypercore/list-markets.rs`)

### Changed

- Improved WebSocket connection handling and reconnection logic

### Fixed

- Fixed issues in example code

### Documentation

- Comprehensive NonceHandler documentation with thread-safety guarantees
- Improved crate-level documentation for docs.rs
- Added "Design choices" section to README
- Enhanced example documentation
- Fixed README examples

### Chore

- Removed Cargo.lock from version control (added to .gitignore)
- Updated CI workflow

**Files Changed**: 25 files, +1377 insertions, -6157 deletions (large reduction due to Cargo.lock removal)
