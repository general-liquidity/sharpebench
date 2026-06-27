/**
 * `@general-liquidity/sharpebench` — the luck-robust scoring kernel for AI trading
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
  FullVerdict,
  GreeksParams,
  GreeksResult,
  HonestyOpts,
  HonestyVerdict,
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

/** Map camelCase {@link HonestyOpts} → the snake_case `HonestyConfig` JSON the kernel reads. */
function honestyConfigJson(opts: HonestyOpts): string {
  const cfg: Record<string, unknown> = { n_trials: opts.nTrials };
  if (opts.trialsSrStd !== undefined) cfg.trials_sr_std = opts.trialsSrStd;
  if (opts.confidence !== undefined) cfg.confidence = opts.confidence;
  if (opts.borderline !== undefined) cfg.borderline = opts.borderline;
  if (opts.srBenchmark !== undefined) cfg.sr_benchmark = opts.srBenchmark;
  return JSON.stringify(cfg);
}

/** Map the kernel's snake_case HonestyVerdict JSON → the camelCase {@link HonestyVerdict}. */
function toHonestyVerdict(raw: Record<string, unknown>): HonestyVerdict {
  return {
    sharpe: raw.sharpe as number,
    nObs: raw.n_obs as number,
    skew: raw.skew as number,
    kurtosis: raw.kurtosis as number,
    nTrials: raw.n_trials as number,
    expectedMaxSharpe: raw.expected_max_sharpe as number,
    deflatedSharpe: raw.deflated_sharpe as number,
    probabilisticSharpe: raw.probabilistic_sharpe as number,
    haircut: raw.haircut as number,
    haircutSharpe: raw.haircut_sharpe as number,
    minTrackRecordLen: raw.min_track_record_len as number,
    verdict: raw.verdict as HonestyVerdict["verdict"],
    explanation: raw.explanation as string,
    methodologyVersion: raw.methodology_version as string,
  };
}

/**
 * "Is my Sharpe real, or an artifact of luck and multiple testing?" — the LITE
 * backtest-honesty verdict over one per-period return series. Deflates the observed
 * Sharpe for `nTrials` (the search footprint), then renders Pass / Borderline / Fail
 * with PSR, expected-max-Sharpe, haircut, and MinTRL.
 */
export function isMySharpeReal(
  returns: number[],
  opts: HonestyOpts,
): HonestyVerdict {
  const raw = parse<Record<string, unknown>>(
    kernel.is_my_sharpe_real(JSON.stringify(returns), honestyConfigJson(opts)),
  );
  return toHonestyVerdict(raw);
}

/**
 * The FULL verdict: the winner's LITE verdict plus the multiple-testing family
 * (White's Reality Check, Hansen's SPA + consistent variant, Romano-Wolf step-down)
 * and the CSCV Probability of Backtest Overfitting over the whole field.
 *
 * `field` is N rows (candidate strategies) × T cols (time); `winnerIdx` is the row
 * whose LITE verdict is reported.
 */
export function isMySharpeRealFull(
  field: number[][],
  winnerIdx: number,
  opts: HonestyOpts,
): FullVerdict {
  const raw = parse<Record<string, unknown>>(
    kernel.is_my_sharpe_real_full(
      JSON.stringify(field),
      winnerIdx,
      honestyConfigJson(opts),
    ),
  );
  return {
    honesty: toHonestyVerdict(raw.honesty as Record<string, unknown>),
    realityCheckP: raw.reality_check_p as number,
    spaP: raw.spa_p as number,
    spaConsistentP: raw.spa_consistent_p as number,
    stepDown: raw.step_down as boolean[],
    pbo: raw.pbo as number,
  };
}
