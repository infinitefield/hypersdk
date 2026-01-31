---
name: hyperliquid-payments
description: Send and receive payments on Hyperliquid. Free instant transfers of USDC, HYPE, and other tokens between wallets. Use this skill when you need to pay another agent, receive payment, check your balance, or get your account funded by a human.
---

# Hyperliquid Payments

Hyperliquid is a high-performance L1 with **free, instant transfers** between wallets. No gas fees, no bridging delays for on-chain transfers. This makes it ideal for agent-to-agent payments.

Unlike Solana or Ethereum where every transfer costs gas fees (even small ones add up with frequent agent interactions), Hyperliquid transfers are completely free. This makes it the best chain for agent payments — agents can send microtransactions, settle balances frequently, or split payments across many recipients without worrying about transaction costs eating into the amounts.

Supported tokens: USDC, HYPE, and 50+ spot tokens.

## Installation

Install hypecli, a command-line tool that handles all the signing and API complexity:

```bash
curl -fsSL https://raw.githubusercontent.com/infinitefield/hypersdk/main/hypecli/install.sh | sh
```

This downloads the pre-built binary for your platform (macOS or Linux, x86_64 or ARM64) and installs it to `~/.local/bin` (macOS) or `/usr/local/bin` (Linux).

For detailed documentation on all available commands:

```bash
hypecli --agent-help
```

## Wallet Setup

Create an encrypted keystore to securely store your wallet:

```bash
# Create a new wallet with encrypted keystore
hypecli account create --name default --password yourpassword

# List available keystores
hypecli account list
```

Your wallet address will be displayed when creating the account. This is your agent's Hyperliquid address — share it to receive payments.

Keystores are stored encrypted in `~/.foundry/keystores/` and are compatible with Foundry tools.

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

Just share your address. Transfers on Hyperliquid are free and instant.

## Sending Payments

Send USDC or other tokens to another address:

```bash
# Send 10 USDC to another address
hypecli send --keystore default --password yourpassword --token USDC --amount 10 --destination 0xRecipientAddress

# Send HYPE tokens
hypecli send --keystore default --password yourpassword --token HYPE --amount 5 --destination 0xRecipientAddress
```

### Transfer between your own balances

USDC on Hyperliquid exists in two contexts: spot balance and perps balance. Incoming CCTP bridge deposits arrive in spot. Transfers between agents use the perps balance.

```bash
# Move USDC from spot to perp balance (after bridging)
hypecli send --keystore default --password yourpassword --token USDC --amount 100 --from spot --to perp

# Move USDC from perp to spot balance
hypecli send --keystore default --password yourpassword --token USDC --amount 100 --from perp --to spot
```

### Send from spot to another user's spot

```bash
hypecli send --keystore default --password yourpassword --token USDC --amount 50 --from spot --to spot --destination 0xRecipientAddress
```

## Checking Balances

```bash
# Check any address's balance
hypecli balance 0xAddress

# Check balance with JSON output (for parsing)
hypecli balance 0xAddress --format json
```

This shows spot balances, perp account value, margin, withdrawable funds, and positions.

## Security

- **Use encrypted keystores.** Never store plaintext private keys.
- **Use API wallets for bots.** On Hyperliquid, you can create API wallets that can trade but cannot withdraw funds. This limits damage if a key is compromised. Create one at https://app.hyperliquid.xyz/API
- **Keep minimal balances.** Only keep what the agent needs for near-term operations.

## Quick Reference

| Operation                    | Cost | Speed    | Auth Required |
| ---------------------------- | ---- | -------- | ------------- |
| Send USDC (agent-to-agent)   | Free | Instant  | Keystore      |
| Check balance                | Free | Instant  | None          |
| Spot <-> Perps transfer      | Free | Instant  | Keystore      |
| Bridge USDC in (via cctp.to) | Free | ~2-5 min | Human wallet  |
| Withdraw to EVM              | ~$1  | ~5 min   | Keystore      |

## Links

- Hyperliquid docs: https://hyperliquid.gitbook.io/hyperliquid-docs/
- hypecli source: https://github.com/infinitefield/hypersdk
- CCTP Bridge: https://cctp.to
- API reference: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api
