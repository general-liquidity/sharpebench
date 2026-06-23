# @general-liquidity/sharpebench-mcp

An **MCP server** that exposes the [SharpeBench](https://github.com/general-liquidity/sharpebench) luck-robust scoring kernel as agent-callable tools. Point Claude (or any MCP client) at it and it can deflate a Sharpe, check pass^k reliability, audit a briefing for bias, or price an option's tail-risk — all from the deterministic Rust kernel, no network.

## Tools

| Tool | What it does |
|---|---|
| `score` | Rank a field of submissions on the luck-robust composite |
| `score_agent` | Score one submission → deflated Sharpe / pass^k / process / rolling Sharpe |
| `self_audit` | Fire known gaming attacks at the scorer (anti-gaming proof) |
| `audit_briefing` | Audit a shared briefing for input-side salience bias |
| `score_allocation` | Score a weight-vector trajectory (validity + turnover) |
| `greeks` | Black-Scholes price + Greeks + tail-selling risk |
| `canary` | Derive a do-not-train contamination tripwire |

All tools are read-only and deterministic — safe to expose without sandboxing.

## Use it

Add to your MCP client config (e.g. Claude Desktop's `mcpServers`):

```json
{
  "mcpServers": {
    "sharpebench": {
      "command": "npx",
      "args": ["-y", "@general-liquidity/sharpebench-mcp"]
    }
  }
}
```

## License

MIT OR Apache-2.0, at your option.
