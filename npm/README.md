# @general-liquidity/sharpebench

**The luck-robust scoring kernel for AI trading agents.**

Rank agents on risk-adjusted *skill that survives deflation*, not the luckiest run over one quarter. This is the **identical Rust kernel** that powers the [SharpeBench](https://github.com/general-liquidity/sharpebench) benchmark, compiled to WebAssembly, with a typed JS/TS API. No native add-on and no network: pure deterministic scoring.

```bash
npm install @general-liquidity/sharpebench
```

## Quickstart

```ts
import { score, greeks, selfAudit } from "@general-liquidity/sharpebench";

// Rank a field. Raw return is reported but is NEVER the rank key. An agent ranks
// only if its edge survives deflation, pass^k reliability, process discipline,
// the stationary-bootstrap null, and the configured mandate. This tiny input
// demonstrates the API shape; use the benchmark's full run geometry for a
// meaningful eligibility verdict.
const board = score([
  { agent_id: "skilled", runs: [{ returns: [0.002, 0.0021, 0.0019, 0.002] }] },
  { agent_id: "lucky",   runs: [{ returns: [0.05, 0, 0, 0] }] },
]);
console.log(board[0].agent_id, board[0].deflated_sharpe, board[0].rank_eligible);

// Run the scorer's built-in checks against its catalogued gaming attacks.
console.log(selfAudit().all_defended); // true

// Options tail-risk: a short-gamma position a linear Sharpe can't see.
console.log(greeks({ spot: 100, strike: 100, t_years: 1, rate: 0.05, vol: 0.2, is_call: true }).price);
```

## API

| Function | Returns |
|---|---|
| `score(submissions, config?)` | ranked `CompositeScore[]` |
| `scoreAgent(submission, config?)` | one `CompositeScore` (deflated Sharpe, pass^k, process, rolling worst-case Sharpe) |
| `selfAudit()` | `SelfAuditReport`, the benchmark's anti-gaming proof |
| `auditBriefing(briefing, policy?)` | `BriefingAudit`, an input-side salience-bias audit |
| `scoreAllocation(trajectory, policy?)` | `AllocationReport`, weight-vector validity plus L1 turnover |
| `greeks(params)` | `GreeksResult`, Black-Scholes price, Greeks, and tail-selling risk |
| `canary(seed)` | `Canary`, a do-not-train contamination tripwire |
| `isMySharpeReal(returns, opts)` | One-series deflation, PSR, haircut, MinTRL, and verdict |
| `isMySharpeRealFull(field, winner, opts)` | Fieldwise Reality Check, SPA, step-down, and PBO alongside the one-series verdict |
| `percentileSelection(candidates, opts?)` | Point winner versus bootstrap-percentile winner and optimism gaps |
| `decomposeUncertainty(input)` | Aleatoric, epistemic, and distributional diagnostic legs |
| `crowdingHalfLife(adoption, params)` | Caller-calibrated crowding-decay prior, reported but never gating |
| `classifyDisqualification(submissions, config?)` | Named hard-gate and advisory reasons |
| `regimeCompare(a, b, regimes, opts?)` | Regime-conditional distribution comparison and pooled-sign reversal |

All inputs and outputs are fully typed (TypeScript declarations ship with the
package). The npm tests compare the WASM package with the native kernel and
committed golden on the Ubuntu CI host. The Rust CI separately pins the two
committed golden fields on Linux, macOS, and Windows; this is not a claim about
every possible input or platform.

## Why luck-robust?

Most agent leaderboards rank a raw Sharpe over a single short window, so they mostly measure noise. SharpeBench gates eligibility on Deflated Sharpe, pass^k reliability across every seed and window, stationary-bootstrap significance, process discipline, and the host drawdown mandate. PSR and the fieldwise multiple-testing family remain visible diagnostics. See the [benchmark repo](https://github.com/general-liquidity/sharpebench) for the full methodology.

## License

MIT OR Apache-2.0, at your option.
