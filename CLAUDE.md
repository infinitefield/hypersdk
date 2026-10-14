# hypersdk

Rust SDK for Hyperliquid. `src/hypercore` is the L1 API (HTTP + WebSocket), `src/hyperevm` is
the EVM side, `hypecli/` is the CLI.

## The SDK's surface

Four enums are the source of truth for what is covered. Read them before anything else.

| Surface | Enum | File |
| --- | --- | --- |
| Info endpoint | `InfoRequest` | `src/hypercore/types/mod.rs` |
| Exchange endpoint | `Action` | `src/hypercore/types/api.rs` |
| WebSocket | `Subscription` | `src/hypercore/types/mod.rs` |
| Deployer actions | `SpotDeployAction`, `PerpDeployAction`, `OutcomeDeployAction` | `src/hypercore/types/deploy.rs` |

## Auditing against the API

Hyperliquid ships endpoints before documenting them, removes them without notice, and leaves
a good deal permanently undocumented. A docs diff alone finds none of that. Do all four steps.

### 1. Run the live audits

These walk the SDK's own surface against the real API and are the only thing that catches a
removal. Run them first: they are cheap and they tell you what is already broken.

```bash
cargo test --lib info_requests_are_still_answered      -- --ignored --nocapture
cargo test --lib subscriptions_are_still_accepted      -- --ignored --nocapture
cargo test --lib undocumented_action_shapes_are_accepted -- --ignored --nocapture
cargo test --lib deployer_action_shapes_are_still_accepted -- --ignored --nocapture
```

The action probes sign with a throwaway key, so nothing can take effect. What matters is which
error comes back. An authorization error ("does not exist", "Must deposit before performing
actions") means the payload parsed. HTTP 422 "Failed to deserialize" means the wire format
drifted. Add a case to the right probe whenever you add an action or subscription.

### 2. Diff the docs against the enums

Fetch pages as raw markdown by appending `.md` to the URL. `llms.txt` lists every page.

```bash
curl -sSL https://hyperliquid.gitbook.io/hyperliquid-docs/llms.txt
curl -sSL https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint.md -o exchange.md

# Exchange actions and subscriptions appear in JSON blocks:
grep -ohE '"type"[[:space:]]*:[[:space:]]*"[A-Za-z0-9_]+"' exchange.md | sort -u

# Info requests appear in tables instead, one row per request:
grep -hE '^\| *type' info-endpoint.md
```

The `for-developers/api/` pages that matter: `info-endpoint`, `info-endpoint/perpetuals`,
`info-endpoint/spot`, `exchange-endpoint`, `websocket/subscriptions`, `priority-fees`,
`deploying-hip-1-and-hip-2-assets`, `hip-3-deployer-actions`, `hip-4-deployer-actions`.

### 3. Cross-check nktkas for undocumented surface

The community TypeScript SDK tracks the API closely and carries a large amount the gitbook
never mentions, plus the EIP-712 type definitions the docs omit. One file per method:

```bash
curl -sSL "https://api.github.com/repos/nktkas/hyperliquid/git/trees/main?recursive=1" -o t.json
grep -oE '"src/api/(exchange|info|subscription)/_methods/[a-zA-Z0-9_]+\.ts"' t.json | sed 's|.*/||;s|\.ts"||' | sort
curl -sSL https://raw.githubusercontent.com/nktkas/hyperliquid/main/src/api/exchange/_methods/<action>.ts
```

Diff that list against the enums. Confirm field order and ABI types here before writing a
`sol!` struct.

### 4. Probe anything new before implementing it

Deserialization happens before signature checking, so a dummy signature is enough to tell a
live shape from a dead one. 422 means the shape is wrong or gone; "Unable to recover signer"
means it parsed.

```bash
SIG='{"r":"0x11...11","s":"0x22...22","v":27}'
curl -sS -X POST https://api.hyperliquid.xyz/exchange -H 'Content-Type: application/json' \
  -d "{\"action\":{\"type\":\"someAction\"},\"nonce\":1,\"signature\":$SIG}"
```

Check both mainnet (`api.hyperliquid.xyz`) and testnet (`api.hyperliquid-testnet.xyz`); they
diverge. For subscriptions, connect to `wss://api.hyperliquid.xyz/ws` and look for a
`subscriptionResponse` versus an `error` frame.

## Things that bite

**Documented does not mean live, and live does not mean documented.** `alignedQuoteTokenInfo`,
`enableAlignedQuoteToken` and `disableAlignedQuoteToken` are documented and rejected on both
networks; do not add them. Meanwhile ~37 actions and info requests the SDK now covers appear
nowhere in the docs. Verify, do not assume.

**Mainnet and testnet drift apart.** HIP-4 outcome deployment moved from
`{"type":"spotDeploy","outcome":{...}}` to a top-level `{"type":"outcomeDeploy",...}` on
testnet while mainnet still parsed the old shape. HIP-4 is testnet-only, so the new shape is
the one that counts. Probe both.

**A dead WebSocket subscription is silent.** The server answers with an `error` frame, which is
why `Incoming::Error` exists. Without checking for it a removed subscription just looks like a
feed that never sends anything, which is how `webData2` stayed in the SDK after removal.

**Signing covers the encoding, not the intent.** L1 actions are signed over `to_vec_named`
msgpack (`utils::rmp_hash`). Adding a field, reordering one, or serializing an optional as
`null` instead of omitting it changes the hash and produces a signature that recovers to the
wrong address. That surfaces as "User or API Wallet 0x... does not exist", which reads like an
account problem but is a serialization bug. Optional fields need
`skip_serializing_if = "Option::is_none"`, and booleans the docs describe as omitted when false
need `skip_serializing_if = "std::ops::Not::not"` (`fast` on cancels, `a` on modifies).

**A wrong `sol!` struct fails the same way.** Field order and ABI types have to match exactly.
The probe catches it: if the error names a *different* address than the msgpack actions signed
by the same key, the EIP-712 definition is wrong.

**Lists of tuples must be sorted before signing.** HIP-3 and HIP-4 deployer actions require
lexicographic order by the first element. Sorting after signing corrupts the request.

**Adding an action means two edits.** The `Action` variant, and its arm in
`Action::signing_typed_data` in `src/hypercore/types/api.rs`, which is the single exhaustive
match deciding msgpack versus EIP-712 for all of `sign`, `sign_sync` and `prehash`.

**A serialization test proves nothing about the endpoint existing.** `alignedQuoteTokenInfo`
had a passing unit test asserting its JSON shape while the endpoint had been removed. Equally,
the live audits parse responses as `serde_json::Value`, so they prove an endpoint answers, not
that a modelled response type still matches. `test_http_undocumented_typed_responses` covers
the modelled ones.

## Before committing

```bash
cargo fmt && cargo clippy -p hypersdk --all-targets && cargo test --lib
cargo test --lib --no-default-features
```

Four `too many arguments` clippy warnings are pre-existing; leave it no worse than you found
it. `hypecli` is a separate crate with a path dependency on this one, so a breaking change
here needs `cd hypecli && cargo check`.
