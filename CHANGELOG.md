# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

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
