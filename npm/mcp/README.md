# @general-liquidity/sharpebench-mcp

An **MCP server** that exposes [SharpeBench](https://github.com/general-liquidity/sharpebench)'s luck-robust quantitative evaluation as agent-callable tools. Point an MCP client at it to deflate a Sharpe, check pass^k reliability, compare regimes, audit a briefing, or price option tail risk. Every result comes from the deterministic Rust kernel with no network access.

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
| `is_my_sharpe_real` | Deflate one return series for its search footprint and render the honesty verdict |
| `regime_compare` | Compare aligned returns inside caller-supplied regimes |
| `percentile_selection` | Compare the observed point winner with a bootstrap-percentile winner |
| `decompose_uncertainty` | Report aleatoric, epistemic, and distributional diagnostic legs |
| `crowding_half_life` | Evaluate a caller-calibrated crowding-decay prior |
| `classify_disqualification` | Name every hard-gate and advisory signal that fired |

All tools are read-only and deterministic. They do not execute entrant code or access the network.

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
