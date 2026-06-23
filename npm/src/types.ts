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
