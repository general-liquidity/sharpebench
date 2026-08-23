// Typed views of the SharpeBench kernel's JSON shapes. Inputs are typed precisely;
// report outputs carry the headline fields plus an index signature, so they stay
// forward-compatible as the kernel adds reported axes.

/** One per-seed × per-window return series + (optional) decision trace/costs. */
export interface Run {
  returns: number[];
  cost?: number;
  confidences?: number[];
  outcomes?: number[];
  trace?: { events: unknown[] };
}

/** An agent's full submission: its runs across seeds × windows. */
export interface AgentSubmission {
  agent_id: string;
  runs: Run[];
  /** The agent's own declared in-sample trials (deflated against). */
  in_sample_trials?: number;
  /** Candidate return series from the agent's own selection search. */
  candidates?: number[][];
}

/** Scoring configuration. Omit (or pass `{}`) to use the luck-robust defaults. */
export interface ScoreConfig {
  n_trials?: number;
  rolling_window?: number;
  [k: string]: unknown;
}

/** A scored agent. Raw return is reported but is never the rank key. */
export interface CompositeScore {
  agent_id: string;
  deflated_sharpe: number;
  passed_k: boolean;
  process_ok: boolean;
  rank_eligible: boolean;
  raw_mean_return: number;
  [k: string]: unknown;
}

export interface SelfAuditReport {
  cases: Array<{ name: string; attack: string; defended: boolean; detail: string }>;
  all_defended: boolean;
}

// --- Briefing-neutrality audit ---------------------------------------------

export type RowKind = "fact" | "uncertainty" | "counterpoint";
export interface BriefingRow {
  text: string;
  kind: RowKind;
}
export interface BriefingSection {
  asset_area: string;
  rows: BriefingRow[];
}
export type TableOrdering = "option_order" | "performance" | "unspecified";
export interface ReturnTable {
  ordering: TableOrdering;
  entries: Array<{ label: string; trailing_return: number }>;
}
export interface Briefing {
  sections: BriefingSection[];
  return_table?: ReturnTable | null;
}
export interface BriefingPolicy {
  max_rows_per_area?: number;
  require_counterbalance?: boolean;
  require_option_order_sort?: boolean;
  max_area_salience?: number;
}
export interface BriefingAudit {
  balanced: boolean;
  violations: unknown[];
  salience: Array<{ asset_area: string; row_count: number; salience: number }>;
}

// --- Allocation-vector scoring ---------------------------------------------

export interface AllocationStep {
  weights: number[];
}
export interface AllocationTrajectory {
  steps: AllocationStep[];
}
export interface AllocationPolicy {
  allow_shorts?: boolean;
  max_gross?: number;
  epsilon?: number;
}
export interface AllocationReport {
  total_turnover: number;
  mean_turnover: number;
  weight_violations: unknown[];
  valid: boolean;
}

// --- Options Greeks ---------------------------------------------------------

export interface GreeksParams {
  spot: number;
  strike: number;
  t_years: number;
  rate: number;
  vol: number;
  is_call: boolean;
}
export interface Greeks {
  delta: number;
  gamma: number;
  theta: number;
  vega: number;
  rho: number;
}
export interface GreeksRisk {
  naked_short_gamma: boolean;
  unbounded_tail: boolean;
  short_vega: boolean;
  net_gamma: number;
  net_vega: number;
}
export interface GreeksResult {
  price: number;
  greeks: Greeks;
  risk: GreeksRisk;
}

// --- Canary -----------------------------------------------------------------

export interface Canary {
  id: string;
  token: string;
}

// --- Backtest-honesty verdict ("is my Sharpe real?") ------------------------

/** The headline call: does the edge survive deflation for the search? */
export type Verdict = "Pass" | "Borderline" | "Fail";

/**
 * Options for {@link isMySharpeReal}. `nTrials` is the multiple-testing footprint
 * (how many strategies/configs were tried before this one was kept) and is the one
 * the caller must think about — `nTrials = 1` is almost always a lie.
 */
export interface HonestyOpts {
  /** Number of strategy trials behind this result. REQUIRED. */
  nTrials: number;
  /** Cross-trial Sharpe dispersion. Omit → estimated at 0.5 and flagged. */
  trialsSrStd?: number;
  /** Deflated-Sharpe threshold for a Pass. Default 0.95. */
  confidence?: number;
  /** Deflated-Sharpe threshold for Borderline. Default 0.90. */
  borderline?: number;
  /** PSR / MinTRL benchmark Sharpe to beat. Default 0.0. */
  srBenchmark?: number;
}

/** The LITE verdict: everything derivable from one return series. */
export interface HonestyVerdict {
  sharpe: number;
  nObs: number;
  skew: number;
  kurtosis: number;
  nTrials: number;
  expectedMaxSharpe: number;
  deflatedSharpe: number;
  probabilisticSharpe: number;
  /** `1 - deflatedSharpe`: probability the edge is a search artifact. */
  haircut: number;
  /** `sharpe * deflatedSharpe`: Sharpe discounted by survival probability. */
  haircutSharpe: number;
  minTrackRecordLen: number;
  verdict: Verdict;
  explanation: string;
  methodologyVersion: string;
  [k: string]: unknown;
}

// --- Percentile selection ----------------------------------------------------

/** What a candidate is scored on inside {@link percentileSelection}. */
export type SelectionUtility = "mean_return" | "sharpe";

/** Options for {@link percentileSelection}. Omit for the recommended defaults. */
export interface PercentileSelectionOpts {
  /** Utility each candidate is scored on. Default `"mean_return"`. */
  utility?: SelectionUtility;
  /**
   * Percentile of the bootstrapped utility distribution to rank on, in [0, 1].
   * Default 0.5 (the middle of the band). Below 0.3 the result carries
   * `alphaWarning: true`: the extreme lower tail is decided by a handful of
   * unlucky resamples. The result is still computed; the warning flags a
   * choice, it does not veto one.
   */
  alpha?: number;
  /** PRNG seed; the result is deterministic given (data, seed). Default 0. */
  seed?: number;
  /** Bootstrap resamples per candidate. Default 2000. */
  nBoot?: number;
  /** Stationary-bootstrap block-restart probability. Default 0.1. */
  blockProb?: number;
}

/** Per-candidate result inside a {@link PercentileSelectionResult}. */
export interface CandidateUtility {
  /** Position of this candidate in the input array. */
  index: number;
  /** Utility on the observed path: the number a naive argmax would rank on. */
  point_utility: number;
  /** The alpha percentile of the bootstrapped utility distribution. */
  percentile_utility: number;
  /** `point_utility - percentile_utility`: how much of the headline number fails to survive resampling. */
  optimism_gap: number;
}

/** Selection on a percentile of a bootstrapped utility distribution. */
export interface PercentileSelectionResult {
  /** The percentile actually used, clamped to [0, 1]. */
  alpha: number;
  /** True when alpha sits below the recommended floor of 0.3. */
  alpha_warning: boolean;
  /** Every candidate, in input order. */
  candidates: CandidateUtility[];
  /** Index of the candidate with the best percentile utility (the robust pick), or null for empty input. */
  selected: number | null;
  /** Index of the candidate with the best point utility (the naive pick), or null for empty input. */
  point_argmax: number | null;
  /** Whether the two picks agree. Disagreement is the interesting case. */
  agrees_with_point_argmax: boolean;
  /** Optimism gap of the point winner: report this next to any headline utility. */
  point_winner_optimism: number;
  [k: string]: unknown;
}

// --- Uncertainty decomposition ----------------------------------------------

/** Inputs for {@link decomposeUncertainty}. Every field is optional; a missing leg's input reads as empty. */
export interface UncertaintyInput {
  /** Realized binary outcomes (true or 1 = the call was right). Drives the aleatoric leg. */
  outcomes?: Array<boolean | number>;
  /** Independent per-decision confidence streams for the same decisions. Drives the epistemic leg. */
  signals?: number[][];
  /** The case's per-period returns. Drives the distributional leg (with the reference). */
  caseReturns?: number[];
  /** The reference per-period returns the case is compared against. */
  referenceReturns?: number[];
}

/**
 * The three legs of uncertainty behind one scored case, each on [0, 1] and
 * reported side by side, never summed. High aleatoric says stop looking, high
 * epistemic says keep looking, high distributional says the case is outside
 * what the reference can speak to.
 */
export interface UncertaintySplit {
  /** Irreducible outcome noise (base-rate variance; 1 = a fair coin). */
  aleatoric: number;
  /** Reducible ignorance, from signal disagreement plus evidence thinness. */
  epistemic: number;
  /** Unlikeness to the reference series (location or dispersion shift). */
  distributional: number;
  /**
   * Load-bearing limitation, spelled out by the kernel: the epistemic leg is a
   * lower bound, never an upper one. Unanimous or correlated signals understate
   * it, so treat only high readings as informative.
   */
  epistemic_caveat: string;
  [k: string]: unknown;
}

// --- Crowding decay prior ----------------------------------------------------

/**
 * Parameters of the crowding decay model. All rates are per period of the
 * caller's IC series; there is deliberately no default calibration, because a
 * stock calibration would smuggle a modelled number in as if it were measured.
 */
export interface CrowdingParams {
  /** Natural mean-reversion rate of the edge at zero adoption. */
  theta: number;
  /** Crowding decay rate at full adoption. */
  deltaMax: number;
  /** Exponent on adoption (1 = linear). Default 1. */
  curvature?: number;
}

/** The expected half-life implied by the crowding model: a prior, not a measurement. */
export interface CrowdingDecayPrior {
  /** Adoption used, clamped to [0, 1]. */
  adoption: number;
  /** theta, echoed back. */
  natural_reversion: number;
  /** The adoption-driven decay component delta(phi). */
  crowding_decay: number;
  /** `ln2 / (theta + delta(phi))` in periods, or null when the model says the edge never decays. */
  expected_half_life: number | null;
  /** Names what this is: a model prior, reported never gating. */
  note: string;
  [k: string]: unknown;
}

// --- Disqualification-reason taxonomy ---------------------------------------

/**
 * A reason an agent was (or should be) demoted. The first five mirror the hard
 * eligibility gates in the scorer; the last three are advisory quality flags
 * that never gate.
 */
export type FailReason =
  | "failed_pass_k"
  | "dsr_below_bar"
  | "process_violation"
  | "bootstrap_insignificant"
  | "mandate_breached"
  | "high_selection_gap"
  | "is_rediscovery"
  | "oos_decay";

/** Every disqualification/quality signal that fired for one scored agent. */
export interface DisqualificationReport {
  agent_id: string;
  rank_eligible: boolean;
  /** Reasons in stable order; empty means no signal fired. */
  reasons: FailReason[];
  [k: string]: unknown;
}

/** The FULL verdict: LITE on the winner plus the multiple-testing family + PBO. */
export interface FullVerdict {
  honesty: HonestyVerdict;
  /** White's Reality Check p-value over the field. */
  realityCheckP: number;
  /** Hansen's SPA p-value (liberal/lower studentized variant). */
  spaP: number;
  /** Hansen's consistent SPA p-value. */
  spaConsistentP: number;
  /** Romano-Wolf step-down: which field members are significant at α. */
  stepDown: boolean[];
  /** CSCV Probability of Backtest Overfitting over the field. */
  pbo: number;
  [k: string]: unknown;
}
