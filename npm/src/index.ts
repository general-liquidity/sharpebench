/**
 * `@generalliquidity/sharpebench` — the luck-robust scoring kernel for AI trading
 * agents, as a typed JS API over the *identical* Rust kernel that powers the
 * SharpeBench benchmark (compiled to WebAssembly).
 *
 * An agent does not rank on raw return. It ranks only if its edge survives
 * deflation for the number of trials, reliability across every seed × window
 * (pass^k), and decision-process discipline. See {@link score}.
 */
import * as kernel from "../pkg/sharpebench.js";

import type {
  AgentSubmission,
  AllocationPolicy,
  AllocationReport,
  AllocationTrajectory,
  Briefing,
  BriefingAudit,
  BriefingPolicy,
  Canary,
  CompositeScore,
  GreeksParams,
  GreeksResult,
  ScoreConfig,
  SelfAuditReport,
} from "./types.js";

export * from "./types.js";

/** Parse a kernel JSON string, surfacing the kernel's `{error}` as a thrown Error. */
function parse<T>(json: string): T {
  const value = JSON.parse(json) as unknown;
  if (value && typeof value === "object" && "error" in value) {
    throw new Error(String((value as { error: unknown }).error));
  }
  return value as T;
}

/** Empty/absent config → `""`, which the kernel reads as "use defaults". */
function optJson(value: object | undefined): string {
  if (!value || Object.keys(value).length === 0) return "";
  return JSON.stringify(value);
}

/**
 * Score and rank a field of submissions on the luck-robust composite. Returns the
 * leaderboard (rank-eligible agents first); raw return is reported but never the
 * rank key.
 */
export function score(
  submissions: AgentSubmission[],
  config?: ScoreConfig,
): CompositeScore[] {
  return parse(kernel.score(JSON.stringify(submissions), optJson(config)));
}

/**
 * Score a single submission → one {@link CompositeScore} carrying its deflated
 * Sharpe, pass^k verdict, process score, rolling worst-case Sharpe, and the rest.
 */
export function scoreAgent(
  submission: AgentSubmission,
  config?: ScoreConfig,
): CompositeScore {
  return parse(kernel.score_agent(JSON.stringify(submission), optJson(config)));
}

/** Fire the known gaming attacks at the scorer and report whether each is demoted. */
export function selfAudit(): SelfAuditReport {
  return parse(kernel.self_audit());
}

/** Audit a shared briefing for input-side salience bias. */
export function auditBriefing(
  briefing: Briefing,
  policy?: BriefingPolicy,
): BriefingAudit {
  return parse(kernel.audit_briefing(JSON.stringify(briefing), optJson(policy)));
}

/** Score a target-allocation trajectory: weight validity + L1 turnover. */
export function scoreAllocation(
  trajectory: AllocationTrajectory,
  policy?: AllocationPolicy,
): AllocationReport {
  return parse(
    kernel.score_allocation(JSON.stringify(trajectory), optJson(policy)),
  );
}

/** Black-Scholes price + Greeks + tail-selling (short-gamma/vega) classification. */
export function greeks(params: GreeksParams): GreeksResult {
  return parse(kernel.greeks(JSON.stringify(params)));
}

/** Derive a deterministic do-not-train contamination tripwire from seed material. */
export function canary(seed: string): Canary {
  return parse(kernel.canary(seed));
}
