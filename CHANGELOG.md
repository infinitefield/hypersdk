# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Added

- `BasicOrder` trigger-order fields from `frontendOpenOrders`: `is_trigger`, `trigger_px`, `trigger_condition`, `is_position_tpsl`
- `OrderResponseStatus::WaitingForTrigger` and `WaitingForFill` order response variants
- Exchange actions `sendToEvmWithData`, `topUpIsolatedOnlyMargin`, `claimRewards`, `authorizeAqav2Role`, and `validatorL1Stream`, with `HttpClient` methods for each
- Deployer actions in the new `hypercore::types::deploy` module: HIP-1/HIP-2 `spotDeploy`, HIP-3 `perpDeploy`, and HIP-4 outcome deployment and settlement, reachable via `HttpClient::spot_deploy`, `perp_deploy`, and `activate_outcome_deployer`
- WebSocket post requests: `Connection::post` and `ConnectionHandle::post` send info requests and signed actions over an open socket, with replies arriving as `Incoming::Post`
- Info requests `userDexAbstraction` and `outcomeTemplates`, via `HttpClient::user_dex_abstraction` and `outcome_templates`
- `fast` flag on `BatchCancel` and `BatchCancelCloid`, serialized as `f` and omitted when false
- Optional `destination` on the `reserveRequestWeight` action, to credit reserved capacity to another account
- New example: `examples/hypercore/websocket_post.rs`
- Two ignored live-audit tests, `info_requests_are_still_answered` and `deployer_action_shapes_are_still_accepted`, that walk the SDK's surface against the real API
- Single-order `modify` action, as `Action::Modify` and `HttpClient::modify_order`. `batchModify` was the only form covered before
- `always_place` on `Modify` and `BatchModify`, serialized as `a` and omitted when false, which places the replacement order even if the cancel failed
- `spotDeploy` variants `setTokenAnnotation` and `setDeployerLabel`
- `outcomeDeploy` variant `setSubDeployers`, which grants a sub-deployer one HIP-4 action
- Eighteen undocumented exchange actions the exchange accepts but the docs do not mention, each with an `HttpClient` method: `borrowLend`, `createSubAccount`, `subAccountModify`, `subAccountTransfer`, `subAccountSpotTransfer`, `createVault`, `vaultModify`, `vaultDistribute`, `setDisplayName`, `setReferrer`, `registerReferrer`, `spotUser`, `finalizeEvmContract`, `CSignerAction`, `CValidatorAction`, `linkStakingUser`, `stakingLinkDisableTradingUser`, and `userPortfolioMargin`
- Nineteen undocumented info requests, each with an `HttpClient` method: `exchangeStatus`, `gossipRootIps`, `isVip`, `leadingVaults`, `legalCheck`, `liquidatable`, `marginTable`, `maxMarketOrderNtls`, `preTransferCheck`, `recentTrades`, `subAccounts2`, `twapHistory`, `usdcRouting`, `userBorrowLendInterest`, `userTwapSliceFillsByTime`, `validatorL1Votes`, `validatorSummaries`, `vaultSummaries`, and `webData2`
- WS subscriptions `assetCtxs`, `spotAssetCtxs`, and `userHistoricalOrders`
- `Incoming::Error`, carrying the `error` channel. A rejected subscription used to be logged and dropped, so a removed subscription looked like a feed that never sent anything
- Live-audit tests `subscriptions_are_still_accepted` and `undocumented_action_shapes_are_accepted`, covering the two surfaces the existing audits missed
- `PredictedFundingVenue::funding_interval_hours`, the funding interval the exchange reports per venue. Optional: 19 of 627 venue payloads on mainnet omit it
- `HttpClient::perp_dex_details`, returning each HIP-3 DEX as `PerpDexDetails`: deployer, oracle updater, fee recipient, sub-deployer permissions as `SubDeployerGrant` (with `SubDeployerPermission` covering `perpDeploy` actions, the `{"hip3Star": ...}` proxy-operation grants of testnet-only HIP-3\* venues, and any other shape kept intact), and per-coin OI caps and funding multipliers, interest rates, and clamps as `AssetSetting`. `perp_dexes` drops everything but the name and index
- New example: `examples/hypercore/hip3-dex-details.rs`
- HIP-3\* support (testnet-only venues with an allowlist and proxied user operations): `PerpDeployAction::Star` with `Hip3StarAction`, whose `Hip3StarOperation` is `setOracle` or a per-user `proxy` operation; `Hip3StarProxyOperation` covers `modifyApproval`, `modifyBackstopLiquidatorApproval`, `setReduceOnly`, `cancel`, `cancelAll`, `order`, and `sendAsset`; `PerpDexSchemaInput::is_star` to create a HIP-3\* venue; and the `userStarState` info request via `HttpClient::user_star_state`. The exchange returns `dexToState` as a list of `[dex, flags]` pairs rather than the object the docs show; `UserStarState` accepts both. Flags are `null` for venues that have since removed the user's approval
- `hypecli dex` commands for HIP-3 deployers: `info` (deployer, sub-deployers, and per-coin OI caps and funding settings), `halt`, `resume`, and `sub-deployer`, plus `allow`, `disallow`, `reduce-only`, `cancel-all`, and `star-state` for testnet-only HIP-3\* venues. Signed commands take the same signer and multisig options as other actions
- Native trailing stops: `Action::TrailingStop` with `TrailingStop` and `TrailingStopRetracement` (a percentage or a price distance), via `HttpClient::trailing_stop`, which returns the order ID from the new `OkResponse::TrailingStop`. An unset `activation_px` is sent as `null`, which the exchange hashes; omitting it recovers a different signer
- `OutcomeInfo::quote_token`, `venue`, and `deployer_fee_scale`, which `outcomeMeta` already returned and the SDK dropped. `venue` is the deployer venue that listed the outcome, such as `out`; the daily "Recurring" markets have none. `OutcomeInfo` also implements `Deserialize`
- `SettledOutcome`, modelling the `settledOutcome` reply: the outcome's `spec`, its `settle_fraction` (quote tokens paid per share of the first side, with the second side receiving the rest), the deployer's `details`, and, for an outcome in a question, a `SettledOutcomeQuestion` whose `OutcomeQuestionState` says whether the question is `Active` or `Settled`. The outcomes of a question can settle one at a time, so an outcome can settle while its question is still active

### Removed

- `HttpClient::aligned_quote_token_info` and `InfoRequest::AlignedQuoteTokenInfo`. The endpoint no longer exists: mainnet and testnet both reject it with the same error they give an unknown request type, and it is absent from the docs
- **Breaking**: `Subscription::WebData2` and `Incoming::WebData2`. The exchange rejects the subscription; `webData3` replaces it. The `webData2` *info* request still works and is now available as `HttpClient::web_data2`

### Changed

- **Breaking**: `HttpClient::predicted_fundings` now returns `Option<PredictedFundingVenue>` for each venue. The exchange sends `null` when a coin is not listed on a venue, which failed to deserialize with `invalid type: null, expected struct PredictedFundingVenue`. On mainnet 69 of 696 venue slots are null, so the endpoint was unusable. `None` means the coin is not listed there, not a zero funding rate
- **Breaking**: `BatchCancel` and `BatchCancelCloid` gained a `fast` field, so struct literals need `fast: false`
- **Breaking**: `BatchModify` gained an `always_place` field, so struct literals need `always_place: false`
- **Breaking**: HIP-4 outcome deployment moved from `SpotDeployAction::Outcome` to its own `Action::OutcomeDeploy`, sent as `{"type": "outcomeDeploy", ...}`. The exchange stopped parsing the old nesting. Use `HttpClient::outcome_deploy`
- **Breaking**: `HttpClient::reserve_request_weight` takes a `destination: Option<Address>` argument
- `Response`, `OkResponse`, `OrderResponseStatus`, and `ActionRequest` now derive `Clone`; `Response`, `OkResponse`, and `OrderResponseStatus` also derive `Serialize`
- `OkResponse` gained `CreateSubAccount` and `CreateVault`, which carry the address the exchange assigns
- **Breaking**: `SubDeployerInput::variant` is a `SubDeployerPermission` instead of a `String`, so it can carry HIP-3\* grants such as `{"hip3Star": "order"}`. `"setOracle".into()` still works
- **Breaking**: `PerpDexSchemaInput` gained an `is_star` field, so struct literals need `is_star: false`
- **Breaking**: HIP-4 `outcomeDeploy` actions carry the deployer's `venue` and nest the variant under `operation`, as the exchange requires: `Action::OutcomeDeploy` and `HttpClient::outcome_deploy` take an `OutcomeDeploy { venue, operation }`. Mainnet and testnet both rejected the old `{"type": "outcomeDeploy", "<variant>": ...}` shape with HTTP 422, so no outcome could be deployed or settled
- All three signing paths (`sign`, `sign_sync`, `prehash`) now share one exhaustive match over `Action`, so adding an action is one edit instead of three
- The live audits `deployer_action_shapes_are_still_accepted` and `undocumented_action_shapes_are_accepted` now also check that the exchange recovers the signing key's address. They only checked that payloads parsed, which is how the signing bugs below went unnoticed
- **Breaking**: `HttpClient::settled_outcome` returns `Option<SettledOutcome>` instead of raw JSON. `None` means the outcome has not settled: the exchange answers `null` both for an outcome that is still trading and for an unknown ID
- **Breaking**: `OutcomeInfo` gained `quote_token`, `venue`, and `deployer_fee_scale` fields, so struct literals need them

### Fixed

- `HttpClient::twap_order` and `twap_cancel` returned an error when the exchange accepted the action: their `twapOrder` and `twapCancel` replies did not deserialize into `Response`. They now parse as `OkResponse::TwapOrder` and `OkResponse::TwapCancel`, carrying `TwapOrderStatus` (the TWAP ID, or the rejection reason) and `TwapCancelStatus`
- `historical_orders` failed for any user or vault whose history held an order with a value the SDK did not know: a trailing stop (order type `"Trailing Stop Market"`), a liquidation (TIF `"LiquidationMarket"`), a vault closing a position on withdrawal (`"Vault Close"`), or an order canceled when a HIP-4 outcome settled (status `outcomeSettledCanceled`). One such order failed the whole response. `OrderType` gains `TrailingStopMarket`, `VaultClose`, `TwapSlice` and `SpotDustConversion`; `TimeInForce` gains `LiquidationMarket`; `OrderStatus` gains `OutcomeSettledCanceled`, `InternalCancel` and `TooManyOpenOrdersRejected`, which `is_cancelled` and `is_rejected` cover
- Signatures on L1 actions that carry an address. `alloy` encodes an `Address` as 20 raw bytes in msgpack while the exchange hashes it as a lowercase hex string, so these actions were signed over different bytes than the exchange hashes and were rejected as "User or API Wallet 0x... does not exist" with an address other than the signer's: `subAccountModify`, `subAccountTransfer`, `subAccountSpotTransfer`, `vaultModify`, `vaultDistribute`, `reserveRequestWeight` with a destination, `CValidatorAction` register and change-profile, and the deployer actions `setSubDeployers` (HIP-3 and HIP-4), `setFeeRecipient`, `registerAsset` and `registerAsset2` with an oracle updater, and `userGenesis`
- `registerAsset` and `registerAsset2` send an unset `maxGas` and `schema` as `null`, which is how the exchange hashes them and how the Python SDK sends them. Omitting either recovered a different signer
- `outcomes` gave both sides of an outcome the second side's asset index unless the first side was named exactly "Yes". Template outcomes name their sides `template:Yes` and `template:No`, or after the participants, so 157 of the 296 outcomes on mainnet mapped their first side onto the second: an order placed with that `OutcomeMarket` traded the other side. Sides are now indexed by their position in `side_specs`

### Dependencies

- Refreshed every dependency to its latest release. `base64` 0.22 -> 0.23 and `hex-literal` 0.4 -> 1 in the SDK, `iroh-mdns-address-lookup` 0.4 -> 0.5 in hypecli, plus a full lockfile update
- **Breaking**: MSRV raised to 1.94.1 on both crates. `alloy` 2.4 already required it, so the declared 1.85.0 had not been buildable for a while and was holding back updates to `serde_with`, `ruint`, `icu_*` and the `aws-*` tree

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
