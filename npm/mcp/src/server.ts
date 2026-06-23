#!/usr/bin/env node
/**
 * SharpeBench MCP server — exposes the luck-robust scoring kernel as
 * Model-Context-Protocol tools, so Claude and other agents can call
 * "deflate this Sharpe / check pass^k / audit this briefing" in their tool loop.
 *
 * Every tool is read-only and deterministic (the kernel has no I/O), so the
 * server is safe to expose without sandboxing.
 */
import { fileURLToPath } from "node:url";

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import * as sb from "@generalliquidity/sharpebench";

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

/** Build the SharpeBench MCP server with all kernel tools registered. */
export function createServer(): McpServer {
  const server = new McpServer({ name: "sharpebench", version: "0.0.3" });

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
