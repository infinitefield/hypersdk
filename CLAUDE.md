# hypersdk

Rust SDK for Hyperliquid. `src/hypercore` is the L1 API (HTTP + WebSocket), `src/hyperevm` is
the EVM side, `hypecli/` is the CLI.

## Auditing the SDK against the API docs

Hyperliquid ships endpoints before documenting them and removes them without notice, so a
docs diff alone is not enough. Do all three of these.

### 1. Diff the SDK's surface against the docs

The SDK's surface lives in three enums. Read them first; they are the source of truth for
what is covered:

- `InfoRequest` in `src/hypercore/types/mod.rs` (info endpoint)
- `Action` in `src/hypercore/types/api.rs` (exchange endpoint)
- `Subscription` in `src/hypercore/types/mod.rs` (WebSocket)

Deployer actions live separately in `src/hypercore/types/deploy.rs`.

Fetch the docs as raw markdown by appending `.md` to any page URL, which avoids scraping
rendered HTML. `llms.txt` lists every page:

```bash
curl -sSL https://hyperliquid.gitbook.io/hyperliquid-docs/llms.txt
curl -sSL https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint.md
```

Then pull the `type` values out and compare against the enums:

```bash
grep -oE '"type"[[:space:]]*:[[:space:]]*"[A-Za-z0-9_]+"' exchange-endpoint.md | sort -u
```

The info-endpoint pages do not use that pattern; list their `## ` headers instead.

### 2. Run the live audits

Two ignored tests walk the SDK's own surface against the real API. They are the only thing
that catches an endpoint the exchange has dropped:

```bash
cargo test --lib info_requests_are_still_answered -- --ignored --nocapture
cargo test --lib deployer_action_shapes_are_still_accepted -- --ignored --nocapture
```

The action probe signs with a throwaway key, so nothing can take effect. What matters is
which error comes back. An authorization error ("does not exist", "Must deposit before
performing actions") means the payload parsed. HTTP 422 "Failed to deserialize" means the
wire format drifted. Add a case to the probe whenever you add an action.

### 3. Cross-check undocumented details against nktkas

The docs omit EIP-712 type definitions for newer user-signed actions. The community
TypeScript SDK carries them and tracks the API closely, so use it to confirm field order and
ABI types before writing a `sol!` struct:

```
https://raw.githubusercontent.com/nktkas/hyperliquid/main/src/api/exchange/_methods/<action>.ts
https://api.github.com/repos/nktkas/hyperliquid/git/trees/main?recursive=1
```

It also covers a good deal of surface the gitbook does not document at all (`borrowLend`,
subaccount and vault management, `finalizeEvmContract`, referrer actions, and a number of
info requests). None of that is in this SDK yet.

## Things that bite

**Documented does not mean live.** As of the August 2026 audit, `alignedQuoteTokenInfo`,
`enableAlignedQuoteToken`, and `disableAlignedQuoteToken` are all in the docs and all
rejected by mainnet and testnet at the JSON parser. Do not add a call the exchange refuses;
verify first.

**Signing covers the encoding, not the intent.** L1 actions are signed over `to_vec_named`
msgpack (`utils::rmp_hash`). Adding a field, reordering one, or serializing an optional as
`null` instead of omitting it changes the hash and produces a signature that recovers to the
wrong address. That surfaces as "User or API Wallet 0x... does not exist", which reads like
an account problem but is a serialization bug. Optional fields need
`skip_serializing_if = "Option::is_none"`, and booleans that the docs describe as omitted
when false need `skip_serializing_if = "std::ops::Not::not"`.

**Lists of tuples must be sorted before signing.** HIP-3 and HIP-4 deployer actions require
lexicographic order by the first element. Sorting after signing corrupts the request.

**Adding an action means four edits, not one.** A new `Action` variant must also be added to
`sign_sync`, `sign`, and `prehash` in `src/hypercore/types/api.rs`. The compiler catches
this because the matches are exhaustive, but they are three separate lists.

**A serialization test proves nothing about the endpoint existing.** `alignedQuoteTokenInfo`
had a passing unit test asserting its JSON shape while the endpoint had been removed.
