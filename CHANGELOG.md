# Changelog

Notable changes to hypersdk and the bundled hypecli, grouped by SDK release tag.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Added

- `Action::user_signed_typed_data`, the EIP-712 typed data a user-signed action is signed over, for signers that need the fields rather than a digest.
- `signing::rmp_hash`, the MessagePack action hash with its nonce, vault and expiry suffix, for callers that hash or verify requests outside `Action`.
- `client` feature, on by default, for the HTTP, WebSocket and HyperEVM clients. With `default-features = false` the crate only models, encodes and signs actions, without reqwest, tokio or the alloy provider stack.

### Fixed

- Updated hypecli's SDK dependency requirement to 0.3.0 so release builds resolve the local SDK.
- User-signed actions and multisig envelopes are signed under the EIP-712 domain their own `signatureChainId` names, as the exchange verifies them. Previously the domain always followed the network, so an action carrying the Python SDK's `0x66eee` on Mainnet got a signature that recovered to another address.
- `convertToMultiSigUser` writes its signers string as the Python SDK does: sorted lowercase addresses with `json.dumps` spacing.

## [v0.3.0] - 2026-10-12

### Added

- Native trailing stops through `HttpClient::trailing_stop`, `Action::TrailingStop`, and `TrailingStopRetracement`, with percentage or price-distance retracement and an optional activation price.
- HIP-3* support for testnet venues: `Hip3StarAction`, oracle updates, proxied user operations, `PerpDexSchemaInput::is_star`, and `HttpClient::user_star_state`.
- `hypecli dex` commands for DEX details, halting and resuming markets, sub-deployer permissions, and HIP-3* account approvals and operations.
- Top-level `hypecli to-normal-user` command for converting a multisig account back to a normal account.
- Typed outcome settlement responses: `SettledOutcome`, `SettledOutcomeQuestion`, and `OutcomeQuestionState`.
- `OutcomeInfo::quote_token`, `venue`, and `deployer_fee_scale`, plus `Deserialize` support.

### Changed

- **Breaking**: `UpdateIsolatedMargin::ntli` and the corresponding `HttpClient::update_isolated_margin` argument are now `i64`. Negative values remove isolated margin.
- **Breaking**: HIP-4 deployment and settlement use `OutcomeDeploy { venue, operation }`. Pass this wrapper to `Action::OutcomeDeploy` or `HttpClient::outcome_deploy`.
- **Breaking**: `HttpClient::settled_outcome` returns `Option<SettledOutcome>` instead of raw JSON. `None` means the outcome has not settled or the ID is unknown.
- **Breaking**: `SubDeployerInput::variant` uses `SubDeployerPermission` instead of `String`, allowing HIP-3* grants. Existing string permissions can use `.into()`.
- **Breaking**: `PerpDexSchemaInput` struct literals need `is_star: false` for ordinary venues; `OutcomeInfo` literals need the new metadata fields.

### Fixed

- Outcome sides are indexed by their position in `side_specs`, so template and participant-named markets receive the correct asset IDs.
- `twap_order` and `twap_cancel` parse successful replies as `OkResponse::TwapOrder` and `OkResponse::TwapCancel`.
- Order history accepts trailing stops, liquidation orders, vault-close orders, TWAP slices, spot dust conversions, and outcome-settlement cancellations. New order statuses are included in `is_cancelled` and `is_rejected`.
- HIP-4 actions include the deployer's venue and nest the operation correctly, fixing rejected deployment and settlement requests.

## [v0.2.18] - 2026-09-28

### Added

- `HttpClient::perp_dex_details` and `PerpDexDetails` expose deployer configuration, sub-deployer grants, open-interest caps, and funding settings. Added `examples/hypercore/hip3-dex-details.rs`.
- Rust SDK skill and reference material for coding agents.

### Changed

- All signing paths (`sign`, `sign_sync`, and `prehash`) share one exhaustive match over `Action`.
- Live action audits verify the recovered signer as well as payload acceptance.
- Unknown sub-deployer permission shapes are preserved instead of failing deserialization.

### Fixed

- L1 actions serialize addresses as lowercase hex strings before msgpack hashing, fixing signatures on subaccount, vault, validator, request-weight, and deployer actions.
- `registerAsset` and `registerAsset2` serialize unset `maxGas` and `schema` as `null`, matching exchange signing bytes.
- Price-tick calculations return `None` for invalid inputs and precision beyond `Decimal` capacity instead of panicking.

## [v0.2.16] - 2026-09-23

### Added

- `PerpMarket::delisted` exposes the exchange's delisting flag.
- `hypecli earn` commands and Trezor typed-data signing, including multisig Earn and Send operations and local address discovery using xpubs.
- Optional `PredictedFundingVenue::funding_interval_hours`.

### Changed

- **Breaking**: `HttpClient::predicted_fundings` returns optional venue payloads. `None` represents a coin that is not listed on that venue.
- **Breaking**: Both crates require Rust 1.94.1, matching the Alloy dependency requirement.
- Unified CLI signer and multisig options across actions.
- Updated SDK and CLI dependencies, including `base64` 0.23, `hex-literal` 1, and `iroh-mdns-address-lookup` 0.5.

### Fixed

- Multisig signing, Trezor EIP-712 domain handling, and Iroh TLS configuration.

## [v0.2.15] - 2026-08-26

### Added

- HIP-1/HIP-2 spot, HIP-3 perpetual, and HIP-4 outcome deployer actions in `hypercore::types::deploy`, with corresponding HTTP client methods.
- Exchange actions for EVM transfers with data, isolated-only margin top-ups, rewards, AQAv2 authorization, and validator L1 streams.
- Single-order `modify`, plus `always_place` on single and batch modifications.
- Exchange actions for borrowing and lending, subaccounts, vault management, referrals, validator and signer management, staking links, and portfolio margin.
- Info requests `userDexAbstraction`, `outcomeTemplates`, and additional queries for exchange status, margin tables, recent trades, subaccounts, TWAP history, validators, vaults, routing, and borrowing interest.
- WebSocket info and signed-action posts through `Connection::post` and `ConnectionHandle::post`, returning `Incoming::Post`. Added a WebSocket post example.
- WebSocket subscriptions `assetCtxs`, `spotAssetCtxs`, and `userHistoricalOrders`, plus `Incoming::Error` for rejected subscriptions.
- Live audits for info requests, exchange actions, deployer actions, and WebSocket subscriptions.
- `hypecli` HIP-4 outcome commands: list, split, merge, merge-question, and negate.

### Changed

- **Breaking**: `BatchCancel` and `BatchCancelCloid` require a `fast` field, omitted from requests when false.
- **Breaking**: `BatchModify` requires an `always_place` field, omitted when false.
- **Breaking**: `HttpClient::reserve_request_weight` accepts `destination: Option<Address>`.
- **Breaking**: Outcome deployment moved from `SpotDeployAction::Outcome` to `Action::OutcomeDeploy` and `HttpClient::outcome_deploy`.
- `Response`, `OkResponse`, `OrderResponseStatus`, and `ActionRequest` derive `Clone`; response types also derive `Serialize`.
- `OkResponse::CreateSubAccount` and `CreateVault` carry the newly assigned address.
- Upgraded hypecli to Alloy 2.

### Fixed

- Market orders use `FrontendMarket` instead of GTC.
- Fetch `deployerFeeScale` per asset.
- Correct JSON mapping for `EvmUserModify` and log WebSocket close frames correctly.

### Removed

- **Breaking**: `HttpClient::aligned_quote_token_info` and `InfoRequest::AlignedQuoteTokenInfo`, which the exchange no longer accepts.
- **Breaking**: `Subscription::WebData2` and `Incoming::WebData2`; use `webData3` for WebSocket data. The `webData2` info request remains available through `HttpClient::web_data2`.

## [v0.2.14] - 2026-06-25

### Added

- HIP-4 `userOutcome` actions for splitting, merging, and negating outcome positions.
- HTTP queries for asset contexts, funding, rate limits, deployer state, token details, settlement, portfolios, referrals, staking, and borrowing and lending.
- TWAP order and cancel, withdrawals, USD class transfers, staking, delegation, request-weight reservation, and HIP-3 liquidator transfers.
- Fast WebSocket asset-context and L2-book subscriptions.
- Spot fee rates and additional account and market response fields.
- Typed, downcastable API and signing errors using `thiserror` and `anyhow`.

### Fixed

- Side-dependent price rounding and reconnect backoff overflow.
- Response field types, including asset-context premium, and compatibility with API fee fields.
- Builder-address handling and CLI compatibility with SDK type changes.

### Dependencies

- Updated Alloy and upgraded hypecli to Iroh 1.0.

## [v0.2.13] - 2026-05-30

### Added

- Builder fees on orders and a builder-fee approval query.
- Typed `FillDirection` and optional token indices in `UserBalance`.
- WebSocket `clearinghouseState`, `allDexsClearinghouseState`, `openOrders`, and `spotState` subscriptions.
- `BasicOrder` trigger metadata: `is_trigger`, `trigger_px`, `trigger_condition`, and `is_position_tpsl`.
- `OrderResponseStatus::WaitingForTrigger` and `WaitingForFill`.
- `hypecli` VWAP support and a user-balances example.

### Changed

- WebSocket background tasks shut down when all handles and streams are dropped.
- Renamed `perp_dexs` to `perp_dexes`, retaining deprecated aliases.
- Added identity-based equality and hashing to market types.

### Fixed

- Spot tokens are matched by their index field instead of array position.
- Market-open price handling and the user-abstraction info request.
- HTTP error handling for non-UTF-8 response bodies and request labels.

### Removed

- Deprecated Uniswap `get_pool_addres` method.

## [v0.2.11] - 2026-04-30

### Added

- `HttpClient::market_open` for native market orders.

### Fixed

- Outcome-market asset IDs include the full outcome offset.
- Restored `n_sig_figs` and `mantissa` on L2-book subscriptions after a temporary removal.

### Changed

- Consolidated `AbstractionMode` into `types/api.rs`.
- Expanded SDK, CLI, and example documentation.

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

- `HttpClient::user_fills_by_time()` to query fills within a time range
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
