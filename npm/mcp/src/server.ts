#!/usr/bin/env node
/**
 * SharpeBench MCP server — exposes the luck-robust scoring kernel as
 * Model-Context-Protocol tools, so Claude and other agents can call
 * "deflate this Sharpe / check pass^k / audit this briefing" in their tool loop.
 *
 * Every tool is read-only and deterministic (the kernel has no I/O), so the
 * server is safe to expose without sandboxing.
 */
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import * as sb from "@general-liquidity/sharpebench";

type ToolResult = { content: Array<{ type: "text"; text: string }>; isError?: boolean };

/** Run a kernel call, returning its result as pretty JSON or a typed error result. */
function run(fn: () => unknown): ToolResult {
  try {
    return { content: [{ type: "text", text: JSON.stringify(fn(), null, 2) }] };
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    return { content: [{ type: "text", text: `error: ${message}` }], isError: true };
  }
}

// The server's version is the package's version, read from package.json at
// load time so a release bump (cargo-release rewrites npm/mcp/package.json from
// the workspace version) is the single source of truth and nothing here drifts.
const { version: PACKAGE_VERSION } = createRequire(import.meta.url)("../package.json") as {
  version: string;
};

/**
 * Regime-conditional comparison lives in `sharpebench-core::regime_compare`.
 * The WASM bridge does not export it yet, so the tool is resolved against the
 * kernel at call time: once `@general-liquidity/sharpebench` ships
 * `regimeCompare`, the tool works unchanged; until then it returns a typed error
 * instead of silently reimplementing the math in TypeScript (one kernel, one
 * answer).
 */
type RegimeCompareFn = (a: number[], b: number[], regimes: string[], opts?: unknown) => unknown;
function kernelRegimeCompare(): RegimeCompareFn {
  const fn = (sb as unknown as { regimeCompare?: RegimeCompareFn }).regimeCompare;
  if (typeof fn !== "function") {
    throw new Error(
      "regime_compare is not exported by this build of @general-liquidity/sharpebench; " +
        "use `sharpebench regime <a.csv> <b.csv> <regimes.csv>` from the CLI",
    );
  }
  return fn;
}

/** Build the SharpeBench MCP server with all kernel tools registered. */
export function createServer(): McpServer {
  const server = new McpServer({ name: "sharpebench", version: PACKAGE_VERSION });

  server.tool(
    "score",
    "Rank a field of agent submissions on the luck-robust composite (deflated Sharpe + pass^k + process discipline). Raw return is reported but is never the rank key. Returns ranked CompositeScore[].",
    { submissions: z.array(z.any()), config: z.any().optional() },
    async ({ submissions, config }) => run(() => sb.score(submissions, config)),
  );

  server.tool(
    "score_agent",
    "Score a single submission → one CompositeScore (deflated Sharpe, pass^k verdict, process score, rolling worst-case Sharpe).",
    { submission: z.any(), config: z.any().optional() },
    async ({ submission, config }) => run(() => sb.scoreAgent(submission, config)),
  );

  server.tool(
    "self_audit",
    "Fire the known gaming attacks at the scorer and report whether each is demoted (the benchmark's anti-gaming proof). No input.",
    async () => run(() => sb.selfAudit()),
  );

  server.tool(
    "audit_briefing",
    "Audit a shared briefing artifact for input-side salience bias: per-asset attention caps, required counterbalancing, no performance-sorted return tables.",
    { briefing: z.any(), policy: z.any().optional() },
    async ({ briefing, policy }) => run(() => sb.auditBriefing(briefing, policy)),
  );

  server.tool(
    "score_allocation",
    "Score a target-allocation weight-vector trajectory: weight validity + L1 turnover/churn.",
    { trajectory: z.any(), policy: z.any().optional() },
    async ({ trajectory, policy }) => run(() => sb.scoreAllocation(trajectory, policy)),
  );

  server.tool(
    "greeks",
    "Black-Scholes price + Greeks (delta/gamma/theta/vega/rho) + tail-selling (short-gamma/vega) classification for one option.",
    {
      spot: z.number(),
      strike: z.number(),
      t_years: z.number(),
      rate: z.number(),
      vol: z.number(),
      is_call: z.boolean(),
    },
    async (params) => run(() => sb.greeks(params)),
  );

  server.tool(
    "canary",
    "Derive a deterministic do-not-train contamination tripwire token from seed material.",
    { seed: z.string() },
    async ({ seed }) => run(() => sb.canary(seed)),
  );

  server.tool(
    "is_my_sharpe_real",
    "Answer 'is this Sharpe real, or an artifact of luck and multiple testing?' for a single return series. Deflates the observed Sharpe for n_trials (the search footprint), then returns a Pass/Borderline/Fail verdict with deflated Sharpe, PSR, haircut, MinTRL, and a plain-English explanation. n_trials = 1 is almost always a lie — pass the true number of strategies/configs you tried.",
    {
      returns: z.array(z.number()),
      n_trials: z.number(),
      trials_sr_std: z.number().optional(),
      confidence: z.number().optional(),
      borderline: z.number().optional(),
      sr_benchmark: z.number().optional(),
    },
    async ({ returns, n_trials, trials_sr_std, confidence, borderline, sr_benchmark }) =>
      run(() =>
        sb.isMySharpeReal(returns, {
          nTrials: n_trials,
          trialsSrStd: trials_sr_std,
          confidence,
          borderline,
          srBenchmark: sr_benchmark,
        }),
      ),
  );

  server.tool(
    "regime_compare",
    "Compare two strategies' per-period returns WITHIN each market regime rather than pooled: per regime, the zero/no-trade mass vs the continuous part (ZAGA split), mean/sd/median, sign share, moment-matched gamma on magnitudes, a two-sample KS statistic, and whether the pooled mean gap hides a sign reversal. Regime labels are an input (one per period); nothing is inferred and no GAMLSS is fitted.",
    {
      returns_a: z.array(z.number()),
      returns_b: z.array(z.number()),
      regimes: z.array(z.string()),
      zero_tol: z.number().optional(),
      min_periods: z.number().optional(),
      tie_tol: z.number().optional(),
    },
    async ({ returns_a, returns_b, regimes, zero_tol, min_periods, tie_tol }) =>
      run(() =>
        kernelRegimeCompare()(returns_a, returns_b, regimes, {
          zeroTol: zero_tol,
          minPeriods: min_periods,
          tieTol: tie_tol,
        }),
      ),
  );

  return server;
}

async function main(): Promise<void> {
  const server = createServer();
  await server.connect(new StdioServerTransport());
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(e);
    process.exit(1);
  });
}
