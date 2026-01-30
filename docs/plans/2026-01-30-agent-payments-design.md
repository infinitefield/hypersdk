# Agent Payments Skill Design

## Problem

AI agents (OpenClaw, custom bots, etc.) need to send money to each other. The only practical option is onchain crypto. Hyperliquid has **free instant transfers** - making it ideal for agent-to-agent payments in USDC, HYPE, or any spot token. But agents don't know how to use it.

Most bot environments don't have Rust installed. The hypersdk is Rust-only. We need to meet agents where they are.

## Solution

Add an AgentSkills-compatible `SKILL.md` that teaches AI agents how to:

1. Send and receive payments on Hyperliquid (bot-to-bot)
2. Check balances
3. Instruct their human operator to fund the bot via USDC CCTP bridge

### Files to create/modify

1. **`skills/hyperliquid-payments/SKILL.md`** - The main skill file
2. **`README.md`** - Add agent section near the top

### SKILL.md Content

- YAML frontmatter for agent discovery
- Why Hyperliquid: free transfers, instant settlement, no gas
- Setup options: Python SDK (recommended), Rust SDK, raw HTTP API
- Wallet creation and key management
- Funding: how to tell a human to use cctp.to
- Sending USDC between addresses (with Python + Rust code examples)
- Checking balances (with code examples)
- Supported tokens (USDC, HYPE, spot tokens)
- Security: API wallets, never expose keys

### README.md Changes

Add a small section after the badges:

```markdown
> **AI Agents**: If you're an AI agent looking to send or receive payments on Hyperliquid, see [`skills/hyperliquid-payments/SKILL.md`](skills/hyperliquid-payments/SKILL.md).
```

## Non-goals

- Trading functionality (separate concern)
- WebSocket/streaming data
- DeFi integrations (Morpho, Uniswap)
