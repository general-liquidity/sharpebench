# @generalliquidity/sharpebench

**The luck-robust scoring kernel for AI trading agents.**

Rank agents on risk-adjusted *skill that survives deflation* — not the luckiest run over one quarter. This is the **identical Rust kernel** that powers the [SharpeBench](https://github.com/general-liquidity/sharpebench) benchmark, compiled to WebAssembly, with a typed JS/TS API. No native add-on, no network — pure deterministic scoring.

```bash
npm install @generalliquidity/sharpebench
```

## Quickstart

```ts
import { score, greeks, selfAudit } from "@generalliquidity/sharpebench";

// Rank a field. Raw return is reported but is NEVER the rank key — an agent ranks
// only if its edge survives deflation, pass^k reliability, and process discipline.
const board = score([
  { agent_id: "skilled", runs: [{ returns: [0.002, 0.0021, 0.0019, 0.002] }] },
  { agent_id: "lucky",   runs: [{ returns: [0.05, 0, 0, 0] }] },
]);
console.log(board[0].agent_id, board[0].deflated_sharpe, board[0].rank_eligible);

// Prove the scorer resists known gaming attacks.
console.log(selfAudit().all_defended); // true

// Options tail-risk: a short-gamma position a linear Sharpe can't see.
console.log(greeks({ spot: 100, strike: 100, t_years: 1, rate: 0.05, vol: 0.2, is_call: true }).price);
```

## API

| Function | Returns |
|---|---|
| `score(submissions, config?)` | ranked `CompositeScore[]` |
| `scoreAgent(submission, config?)` | one `CompositeScore` (deflated Sharpe, pass^k, process, rolling worst-case Sharpe) |
| `selfAudit()` | `SelfAuditReport` — the benchmark's anti-gaming proof |
| `auditBriefing(briefing, policy?)` | `BriefingAudit` — input-side salience-bias audit |
| `scoreAllocation(trajectory, policy?)` | `AllocationReport` — weight-vector validity + L1 turnover |
| `greeks(params)` | `GreeksResult` — Black-Scholes price + Greeks + tail-selling risk |
| `canary(seed)` | `Canary` — a do-not-train contamination tripwire |

All inputs and outputs are fully typed (TypeScript declarations ship with the package). The kernel is deterministic: the same input yields byte-identical scores on any platform.

## Why luck-robust?

Most agent leaderboards rank a raw Sharpe over a single short window — so they mostly measure noise. SharpeBench adds, as **ranking gates**: deflated Sharpe / PSR, pass^k reliability (clear the bar on *every* seed × window), field-wide significance, and process discipline (an order that skipped the risk gate zeroes the entry). See the [benchmark repo](https://github.com/general-liquidity/sharpebench) for the full methodology.

## License

MIT OR Apache-2.0, at your option.
