//! Typed run identity for a submitted field.
//!
//! A submission is a `Vec<Run>`. Nothing in that array says which window, seed
//! or period each run belongs to, so every cross-agent comparison in
//! [`composite`](crate::composite) reaches for `runs[i]` and trusts that agent
//! A's index `i` and agent B's index `i` name the same cell. That trust is not
//! established anywhere: a field assembled from two producers, or one where a
//! single cell failed and was dropped, silently compares different windows.
//!
//! This module makes the cell an explicit, typed key instead. A keyed field
//! declares, per agent, one [`RunKey`] per run; parsing validates that
//!
//! 1. every submission is keyed (an unkeyed legacy array is **refused**, never
//!    aligned by position),
//! 2. no agent repeats a key,
//! 3. every agent carries exactly the same key set,
//! 4. that key set is the complete Cartesian product of the observed windows
//!    and seeds, with no missing and no unexpected cell, and
//! 5. declared period identities match the run's return length, name no
//!    period twice within a run, and agree across agents for the same cell,
//!    and
//! 6. no period is declared in two different windows. Execution seeds of one
//!    window replicate the same market periods and may share them; windows are
//!    the successive segments the pooled track concatenates, so a period in two
//!    of them would enter that track twice, and
//! 7. every agent lists the windows in the same order. That order is the one
//!    the pooled track concatenates them in, so it is the field's time axis and
//!    the submission has to declare it. A window identity is opaque, so its
//!    spelling says nothing about when it ran: ordering the segments by label
//!    would make `max_drawdown` and `return_drift_half_life` change when
//!    `w1 .. w12` is renamed `w01 .. w12`. Agents that disagree declare no
//!    order, and the field is refused rather than settled by the labels.
//!
//! A repeated period is refused per run, before any cross-agent comparison, so
//! a field in which every agent repeats the same period is refused too: the
//! agents agreeing does not make one period two observations.
//!
//! Only then are the runs reordered into one canonical key order, so the
//! positional reads downstream are keyed reads by construction.
//!
//! Completeness is a safety property, not a convenience. Restriction to shared
//! support (see [`comparison_sets`](crate::comparison_sets)) means a peer that
//! submits a partial field shrinks the support every other entrant is judged
//! on, which can drop the very cell that carried another entrant's process
//! violation. Scoring a partial grid must therefore fail loudly rather than
//! quietly rescope the comparison.
//!
//! Pure and deterministic: no I/O, no clock, no ambient randomness. Structural
//! validation only; nothing here rescores or revalidates an outcome.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::composite::{parse_declared_field, AgentSubmission, MandateDeclarations};

/// The cell one run was produced on: a window identity and an execution seed.
///
/// `window` is an opaque exact-match identity (e.g. `"2025-Q4"`), not an index,
/// so a reordered or partially assembled field cannot be reinterpreted. Order
/// is `(window, seed)` so a canonical key order is stable across producers.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RunKey {
    pub window: String,
    pub seed: u64,
}

impl fmt::Display for RunKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(window `{}`, seed {})", self.window, self.seed)
    }
}

/// One declared run identity: its cell, plus optionally the period identities
/// the run's returns are indexed by.
///
/// `periods` is empty when the submitter declares no period axis. When it is
/// present it must have one entry per return, no entry twice, and two agents
/// on the same cell must declare the same periods.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunIdentity {
    #[serde(flatten)]
    pub key: RunKey,
    #[serde(default)]
    pub periods: Vec<String>,
}

/// The `run_keys` sidecar as it arrives on the wire, beside the ordinary
/// submission keys. Unknown fields are ignored on both sides, so a keyed field
/// still deserializes as a plain `Vec<AgentSubmission>` for readers that do not
/// ask for identity, and an unkeyed field parses here with `run_keys` absent
/// (which [`parse_keyed_field`] then refuses).
#[derive(Clone, Debug, Deserialize)]
struct RunKeysDoc {
    agent_id: String,
    run_keys: Option<Vec<RunIdentity>>,
}

/// Why a field could not be accepted as keyed. Every variant names the missing
/// or conflicting identity; none of them is recoverable by guessing an
/// alignment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunIdentityError {
    /// The submissions JSON did not parse, or agent identities were invalid.
    InvalidField(String),
    /// The field has no submissions, so there is no grid to validate.
    EmptyField,
    /// A legacy unkeyed `Run` array. Refused rather than aligned by position.
    UnkeyedSubmission { agent_id: String },
    /// `run_keys` and `runs` disagree in length.
    KeyCountMismatch {
        agent_id: String,
        runs: usize,
        keys: usize,
    },
    /// A window identity that cannot be compared exactly.
    InvalidKey {
        agent_id: String,
        index: usize,
        reason: String,
    },
    /// One agent declared the same cell twice.
    DuplicateKey { agent_id: String, key: RunKey },
    /// An agent is missing a cell another agent submitted.
    MissingCell { agent_id: String, key: RunKey },
    /// The submitted cells are not the complete window times seed product.
    IncompleteGrid {
        windows: usize,
        seeds: usize,
        submitted: usize,
        first_missing: RunKey,
    },
    /// Declared period identities do not index the run's returns.
    PeriodCountMismatch {
        agent_id: String,
        key: RunKey,
        periods: usize,
        returns: usize,
    },
    /// One run declared the same period identity at two indices, so that
    /// period's return would count as two observations in every statistic
    /// computed from the run.
    DuplicatePeriod {
        agent_id: String,
        key: RunKey,
        period: String,
        first: usize,
        second: usize,
    },
    /// Two different windows declare the same period identity, so the pooled
    /// track, which concatenates the windows, would count that period's return
    /// twice. Seeds of one window may share periods.
    PeriodInTwoWindows(Box<PeriodOverlap>),
    /// Two agents declared different period identities for the same cell.
    PeriodMismatch {
        key: RunKey,
        agent_id: String,
        other_agent_id: String,
        index: usize,
    },
    /// Two agents listed the same windows in different orders, so the field
    /// does not say which order the pooled track concatenates them in.
    WindowOrderMismatch {
        agent_id: String,
        other_agent_id: String,
        windows: Vec<String>,
        other_windows: Vec<String>,
    },
}

/// The two cells of [`RunIdentityError::PeriodInTwoWindows`]. `first` is the
/// earlier cell in canonical order; each cell names the agent whose declaration
/// recorded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeriodOverlap {
    pub period: String,
    pub first: RunKey,
    pub first_agent_id: String,
    pub second: RunKey,
    pub second_agent_id: String,
}

impl fmt::Display for RunIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidField(error) => write!(f, "{error}"),
            Self::EmptyField => write!(f, "run identity: the field contains no submissions"),
            Self::UnkeyedSubmission { agent_id } => write!(
                f,
                "run identity: agent `{agent_id}` submitted an unkeyed run array; \
                 add a `run_keys` entry per run (window and seed). Runs are not \
                 aligned across agents by position"
            ),
            Self::KeyCountMismatch {
                agent_id,
                runs,
                keys,
            } => write!(
                f,
                "run identity: agent `{agent_id}` declared {keys} run key(s) for {runs} run(s)"
            ),
            Self::InvalidKey {
                agent_id,
                index,
                reason,
            } => write!(
                f,
                "run identity: agent `{agent_id}` run key {index} is invalid: {reason}"
            ),
            Self::DuplicateKey { agent_id, key } => write!(
                f,
                "run identity: agent `{agent_id}` declared {key} more than once"
            ),
            Self::MissingCell { agent_id, key } => write!(
                f,
                "run identity: agent `{agent_id}` is missing {key}, which another \
                 agent submitted. A partial field cannot be scored: restricting to \
                 shared support would rescope every other entrant's evidence"
            ),
            Self::IncompleteGrid {
                windows,
                seeds,
                submitted,
                first_missing,
            } => write!(
                f,
                "run identity: incomplete grid, {submitted} cell(s) submitted for \
                 {windows} window(s) times {seeds} seed(s); first missing {first_missing}"
            ),
            Self::PeriodCountMismatch {
                agent_id,
                key,
                periods,
                returns,
            } => write!(
                f,
                "run identity: agent `{agent_id}` {key} declared {periods} period \
                 identity/identities for {returns} return(s)"
            ),
            Self::DuplicatePeriod {
                agent_id,
                key,
                period,
                first,
                second,
            } => write!(
                f,
                "run identity: agent `{agent_id}` {key} declares period `{period}` at \
                 index {first} and again at index {second}; one period cannot contribute \
                 two returns to a run"
            ),
            Self::PeriodInTwoWindows(overlap) => {
                let PeriodOverlap {
                    period,
                    first,
                    first_agent_id,
                    second,
                    second_agent_id,
                } = overlap.as_ref();
                write!(
                    f,
                    "run identity: period `{period}` is declared in {first} by agent \
                 `{first_agent_id}` and in {second} by agent `{second_agent_id}`; a \
                 period belongs to one window, and the pooled track would count its \
                 return twice. Seeds of one window may share periods, different \
                 windows may not"
                )
            }
            Self::PeriodMismatch {
                key,
                agent_id,
                other_agent_id,
                index,
            } => write!(
                f,
                "run identity: {key} has different period identities at index {index} \
                 for agents `{other_agent_id}` and `{agent_id}`"
            ),
            Self::WindowOrderMismatch {
                agent_id,
                other_agent_id,
                windows,
                other_windows,
            } => write!(
                f,
                "run identity: agent `{other_agent_id}` lists windows {other_windows:?} \
                 and agent `{agent_id}` lists {windows:?}. The declared order is the \
                 order the pooled track concatenates them in, so the field must not \
                 leave it to the labels' spelling"
            ),
        }
    }
}

impl std::error::Error for RunIdentityError {}

/// A field whose runs are identified rather than positioned: every submission
/// carries the same complete set of cells, and its runs are reordered into the
/// shared canonical `keys` order.
#[derive(Clone, Debug)]
pub struct KeyedField {
    /// The canonical cell order: window-major in `windows` order, seeds
    /// ascending. Index `i` of every submission's `runs` is `keys[i]` for every
    /// agent.
    pub keys: Vec<RunKey>,
    /// The distinct window identities, in the order every agent declared them.
    /// This is the field's time axis, not a sort of the labels.
    pub windows: Vec<String>,
    /// The distinct execution seeds, sorted.
    pub seeds: Vec<u64>,
    /// Submissions in input order, runs reordered into `keys` order.
    pub submissions: Vec<AgentSubmission>,
    /// Mandate declarations, unchanged from [`parse_declared_field`].
    pub declarations: MandateDeclarations,
}

/// The windows in the order `identities` first names each one. This is the
/// field's declared time axis: the order the pooled track concatenates its
/// segments in, taken from the submission rather than from the labels.
fn declared_window_order(identities: &[RunIdentity]) -> Vec<String> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut order = Vec::new();
    for identity in identities {
        if seen.insert(identity.key.window.as_str()) {
            order.push(identity.key.window.clone());
        }
    }
    order
}

fn check_window(window: &str) -> Result<(), String> {
    if window.is_empty() {
        return Err("window identity is empty".to_string());
    }
    if window.trim() != window {
        return Err(format!(
            "window identity `{window}` has leading or trailing whitespace, so exact \
             comparison across producers is unsafe"
        ));
    }
    if window.chars().any(char::is_control) {
        return Err(format!(
            "window identity `{window}` contains a control character"
        ));
    }
    Ok(())
}

/// The indices of the first period identity that appears twice, as
/// `(first, second)`, where `second` is the earliest index that repeats an
/// earlier entry.
fn first_repeated_period(periods: &[String]) -> Option<(usize, usize)> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (second, period) in periods.iter().enumerate() {
        if let Some(first) = seen.insert(period.as_str(), second) {
            return Some((first, second));
        }
    }
    None
}

/// Parse a submissions field and require typed run identity throughout.
///
/// Refuses a legacy unkeyed array, a partial grid, a duplicated cell, a period
/// repeated within a run, disagreeing period identities, and a period declared
/// in two windows. On success the
/// returned submissions are reordered so positional access downstream is
/// keyed access.
pub fn parse_keyed_field(json: &str) -> Result<KeyedField, RunIdentityError> {
    let (subs, declarations) =
        parse_declared_field(json).map_err(RunIdentityError::InvalidField)?;
    let docs: Vec<RunKeysDoc> = serde_json::from_str(json)
        .map_err(|error| RunIdentityError::InvalidField(format!("invalid run keys: {error}")))?;
    if subs.is_empty() {
        return Err(RunIdentityError::EmptyField);
    }

    // Per-agent identity: keyed, well formed, no repeats, one key per run.
    let mut per_agent: Vec<(usize, Vec<RunIdentity>)> = Vec::with_capacity(subs.len());
    for (position, sub) in subs.iter().enumerate() {
        let doc = docs
            .iter()
            .find(|d| d.agent_id == sub.agent_id)
            .ok_or_else(|| RunIdentityError::UnkeyedSubmission {
                agent_id: sub.agent_id.clone(),
            })?;
        let identities =
            doc.run_keys
                .clone()
                .ok_or_else(|| RunIdentityError::UnkeyedSubmission {
                    agent_id: sub.agent_id.clone(),
                })?;
        if identities.len() != sub.runs.len() {
            return Err(RunIdentityError::KeyCountMismatch {
                agent_id: sub.agent_id.clone(),
                runs: sub.runs.len(),
                keys: identities.len(),
            });
        }
        let mut seen = BTreeSet::new();
        for (index, identity) in identities.iter().enumerate() {
            check_window(&identity.key.window).map_err(|reason| RunIdentityError::InvalidKey {
                agent_id: sub.agent_id.clone(),
                index,
                reason,
            })?;
            if !seen.insert(identity.key.clone()) {
                return Err(RunIdentityError::DuplicateKey {
                    agent_id: sub.agent_id.clone(),
                    key: identity.key.clone(),
                });
            }
            if !identity.periods.is_empty()
                && identity.periods.len() != sub.runs[index].returns.len()
            {
                return Err(RunIdentityError::PeriodCountMismatch {
                    agent_id: sub.agent_id.clone(),
                    key: identity.key.clone(),
                    periods: identity.periods.len(),
                    returns: sub.runs[index].returns.len(),
                });
            }
            if let Some((first, second)) = first_repeated_period(&identity.periods) {
                return Err(RunIdentityError::DuplicatePeriod {
                    agent_id: sub.agent_id.clone(),
                    key: identity.key.clone(),
                    period: identity.periods[second].clone(),
                    first,
                    second,
                });
            }
        }
        per_agent.push((position, identities));
    }

    // The union of submitted cells is the grid every agent must cover.
    let mut union: BTreeSet<RunKey> = BTreeSet::new();
    for (_, identities) in &per_agent {
        union.extend(identities.iter().map(|i| i.key.clone()));
    }
    for (position, identities) in &per_agent {
        let held: BTreeSet<&RunKey> = identities.iter().map(|i| &i.key).collect();
        if let Some(missing) = union.iter().find(|key| !held.contains(key)) {
            return Err(RunIdentityError::MissingCell {
                agent_id: subs[*position].agent_id.clone(),
                key: missing.clone(),
            });
        }
    }

    // The window axis is ordered by declaration, not by spelling. The pooled
    // track concatenates the windows as successive segments (rule 6), so their
    // order sets the field's time axis, and `max_drawdown` and
    // `return_drift_half_life` are read off that track. Taking the order from a
    // `BTreeSet<RunKey>` would take it from the labels: `w1 .. w12` sorts as
    // `w1, w10, w11, w12, w2, ..`, and respelling them `w01 .. w12` would move
    // the drawdown without moving a single return. Each agent declares the
    // order by the order it first names each window, and a field whose agents
    // disagree names no order at all, so it is refused rather than settled by
    // the labels.
    let (first_position, first_identities) = &per_agent[0];
    let windows = declared_window_order(first_identities);
    for (position, identities) in &per_agent {
        let declared = declared_window_order(identities);
        if declared != windows {
            return Err(RunIdentityError::WindowOrderMismatch {
                agent_id: subs[*position].agent_id.clone(),
                other_agent_id: subs[*first_position].agent_id.clone(),
                windows: declared,
                other_windows: windows,
            });
        }
    }

    // Completeness: the union must be the whole window times seed product. No
    // axis is inferred beyond the identities actually submitted.
    let seeds: Vec<u64> = union
        .iter()
        .map(|key| key.seed)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if union.len() != windows.len() * seeds.len() {
        let first_missing = windows
            .iter()
            .flat_map(|window| {
                seeds.iter().map(move |seed| RunKey {
                    window: window.clone(),
                    seed: *seed,
                })
            })
            .find(|key| !union.contains(key))
            .expect("a short product has at least one missing cell");
        return Err(RunIdentityError::IncompleteGrid {
            windows: windows.len(),
            seeds: seeds.len(),
            submitted: union.len(),
            first_missing,
        });
    }

    // Period identities must describe the cell, not the submitter.
    let mut declared_periods: BTreeMap<RunKey, (String, Vec<String>)> = BTreeMap::new();
    for (position, identities) in &per_agent {
        let agent_id = &subs[*position].agent_id;
        for identity in identities {
            if identity.periods.is_empty() {
                continue;
            }
            match declared_periods.get(&identity.key) {
                None => {
                    declared_periods.insert(
                        identity.key.clone(),
                        (agent_id.clone(), identity.periods.clone()),
                    );
                }
                Some((other_agent_id, periods)) => {
                    if let Some(index) = periods
                        .iter()
                        .zip(&identity.periods)
                        .position(|(a, b)| a != b)
                        .or_else(|| {
                            (periods.len() != identity.periods.len())
                                .then(|| periods.len().min(identity.periods.len()))
                        })
                    {
                        return Err(RunIdentityError::PeriodMismatch {
                            key: identity.key.clone(),
                            agent_id: agent_id.clone(),
                            other_agent_id: other_agent_id.clone(),
                            index,
                        });
                    }
                }
            }
        }
    }

    // A period belongs to one window. `declared_periods` holds every cell that
    // any agent gave periods for, and the check above made those declarations
    // agree, so this covers one agent repeating a period across its windows and
    // two agents whose separate declarations do.
    let mut period_window: BTreeMap<&str, (&RunKey, &str)> = BTreeMap::new();
    for (key, (agent_id, periods)) in &declared_periods {
        for period in periods {
            let (first, first_agent_id) = *period_window
                .entry(period.as_str())
                .or_insert((key, agent_id.as_str()));
            if first.window != key.window {
                return Err(RunIdentityError::PeriodInTwoWindows(Box::new(
                    PeriodOverlap {
                        period: period.clone(),
                        first: first.clone(),
                        first_agent_id: first_agent_id.to_string(),
                        second: key.clone(),
                        second_agent_id: agent_id.clone(),
                    },
                )));
            }
        }
    }

    // Canonical order, applied to every submission: window-major in the
    // declared window order, seeds ascending. Completeness above makes this the
    // same set as `union`, ordered by the field's time axis rather than by the
    // labels.
    let keys: Vec<RunKey> = windows
        .iter()
        .flat_map(|window| {
            seeds.iter().map(move |seed| RunKey {
                window: window.clone(),
                seed: *seed,
            })
        })
        .collect();
    let mut submissions = subs;
    for (position, identities) in per_agent {
        let mut by_key: BTreeMap<&RunKey, usize> = BTreeMap::new();
        for (index, identity) in identities.iter().enumerate() {
            by_key.insert(&identity.key, index);
        }
        let order: Vec<usize> = keys
            .iter()
            .map(|key| *by_key.get(key).expect("every cell checked present"))
            .collect();
        let runs = std::mem::take(&mut submissions[position].runs);
        let mut slots: Vec<Option<_>> = runs.into_iter().map(Some).collect();
        submissions[position].runs = order
            .into_iter()
            .map(|index| {
                slots[index]
                    .take()
                    .expect("permutation visits each run once")
            })
            .collect();
    }

    Ok(KeyedField {
        keys,
        windows,
        seeds,
        submissions,
        declarations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{score_agent, ScoreConfig};

    /// One agent, one seed, one return per window, windows listed in the given
    /// order. Only the labels differ between two calls with the same returns.
    fn spelled_field(windows: &[String], returns: &[f64]) -> String {
        let docs = serde_json::json!([{
            "agent_id": "a",
            "runs": returns
                .iter()
                .map(|r| serde_json::json!({ "returns": [r] }))
                .collect::<Vec<_>>(),
            "run_keys": windows
                .iter()
                .map(|w| serde_json::json!({ "window": w, "seed": 0 }))
                .collect::<Vec<_>>(),
        }]);
        serde_json::to_string(&docs).expect("serialize field")
    }

    /// S2: the pooled track concatenates the windows in the order they are
    /// declared, so a statistic read off it cannot change when the labels are
    /// respelled. `w1 .. w12` sorts lexicographically as `w1, w10, w11, w12,
    /// w2, ..`, and `w01 .. w12` sorts into its declared order, so ordering the
    /// canonical keys by label made `max_drawdown` a function of the spelling.
    #[test]
    fn a_pooled_statistic_does_not_change_when_the_window_labels_are_respelled() {
        // Two drops first, then a long recovery. Read in declared order the
        // drops compound; read in label order the recovery is spliced between
        // them and the deepest drawdown is shallower.
        let mut returns = vec![-0.3, -0.3];
        returns.extend(std::iter::repeat_n(0.4, 10));
        let short: Vec<String> = (1..=12).map(|i| format!("w{i}")).collect();
        let padded: Vec<String> = (1..=12).map(|i| format!("w{i:02}")).collect();

        let cfg = ScoreConfig {
            execution_seeds_per_window: 1,
            ..ScoreConfig::default()
        };
        let drawdown = |windows: &[String]| {
            let keyed = parse_keyed_field(&spelled_field(windows, &returns)).expect("keyed field");
            score_agent(&keyed.submissions[0], &cfg).max_drawdown
        };

        assert_eq!(
            drawdown(&short),
            drawdown(&padded),
            "renaming w1..w12 to w01..w12 changed no return, so it must change no statistic"
        );
    }

    /// The order the windows are declared in is the field's time axis, so two
    /// agents that disagree about it describe two different pooled tracks.
    /// Nothing else in a keyed field can settle which one is right.
    #[test]
    fn agents_that_declare_different_window_orders_are_refused() {
        let json = field(&[
            ("a", &[("w0", 0), ("w1", 0)]),
            ("b", &[("w1", 0), ("w0", 0)]),
        ]);
        let error = parse_keyed_field(&json).expect_err("contradictory window order");
        assert!(
            matches!(error, RunIdentityError::WindowOrderMismatch { .. }),
            "{error}"
        );
    }

    fn field(entries: &[(&str, &[(&str, u64)])]) -> String {
        let docs: Vec<serde_json::Value> = entries
            .iter()
            .map(|(agent_id, cells)| {
                serde_json::json!({
                    "agent_id": agent_id,
                    "runs": cells
                        .iter()
                        .map(|(window, seed)| serde_json::json!({
                            "returns": [0.01, 0.02, *seed as f64 * 0.001, window.len() as f64 * 0.001],
                        }))
                        .collect::<Vec<_>>(),
                    "run_keys": cells
                        .iter()
                        .map(|(window, seed)| serde_json::json!({
                            "window": window,
                            "seed": seed,
                        }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        serde_json::to_string(&docs).expect("serialize field")
    }

    #[test]
    fn a_complete_keyed_grid_is_reordered_into_one_canonical_cell_order() {
        // The two agents interleave the same four cells differently. Position
        // alignment would compare different cells; keyed alignment must not.
        // They agree on which window comes first, which is the one part of the
        // listing order that carries meaning.
        let json = field(&[
            ("a", &[("w0", 0), ("w0", 1), ("w1", 0), ("w1", 1)]),
            ("b", &[("w0", 1), ("w1", 0), ("w0", 0), ("w1", 1)]),
        ]);
        let keyed = parse_keyed_field(&json).expect("complete grid");
        assert_eq!(
            keyed.keys,
            vec![
                RunKey {
                    window: "w0".into(),
                    seed: 0
                },
                RunKey {
                    window: "w0".into(),
                    seed: 1
                },
                RunKey {
                    window: "w1".into(),
                    seed: 0
                },
                RunKey {
                    window: "w1".into(),
                    seed: 1
                },
            ]
        );
        assert_eq!(keyed.windows, vec!["w0".to_string(), "w1".to_string()]);
        assert_eq!(keyed.seeds, vec![0, 1]);
        for index in 0..keyed.keys.len() {
            assert_eq!(
                keyed.submissions[0].runs[index].returns, keyed.submissions[1].runs[index].returns,
                "cell {index} must hold the same window and seed for both agents"
            );
        }
    }

    #[test]
    fn a_legacy_unkeyed_run_array_is_refused_not_positionally_aligned() {
        let json = r#"[{"agent_id":"a","runs":[{"returns":[0.01,0.02]}]}]"#;
        assert_eq!(
            parse_keyed_field(json).unwrap_err(),
            RunIdentityError::UnkeyedSubmission {
                agent_id: "a".to_string()
            }
        );
        assert!(parse_keyed_field(json)
            .unwrap_err()
            .to_string()
            .contains("run_keys"));
    }

    #[test]
    fn a_missing_cell_fails_instead_of_scoring_a_partial_field() {
        let json = field(&[
            ("a", &[("w0", 0), ("w0", 1), ("w1", 0), ("w1", 1)]),
            ("b", &[("w0", 0), ("w0", 1), ("w1", 0)]),
        ]);
        assert_eq!(
            parse_keyed_field(&json).unwrap_err(),
            RunIdentityError::MissingCell {
                agent_id: "b".to_string(),
                key: RunKey {
                    window: "w1".into(),
                    seed: 1
                },
            }
        );
    }

    #[test]
    fn an_incomplete_cartesian_grid_fails_even_when_every_agent_agrees() {
        let json = field(&[
            ("a", &[("w0", 0), ("w0", 1), ("w1", 0)]),
            ("b", &[("w0", 0), ("w0", 1), ("w1", 0)]),
        ]);
        assert_eq!(
            parse_keyed_field(&json).unwrap_err(),
            RunIdentityError::IncompleteGrid {
                windows: 2,
                seeds: 2,
                submitted: 3,
                first_missing: RunKey {
                    window: "w1".into(),
                    seed: 1
                },
            }
        );
    }

    #[test]
    fn a_duplicated_cell_fails_instead_of_inflating_the_grid() {
        let json = field(&[("a", &[("w0", 0), ("w0", 0)])]);
        assert_eq!(
            parse_keyed_field(&json).unwrap_err(),
            RunIdentityError::DuplicateKey {
                agent_id: "a".to_string(),
                key: RunKey {
                    window: "w0".into(),
                    seed: 0
                },
            }
        );
    }

    #[test]
    fn key_count_must_match_run_count() {
        let json = serde_json::json!([{
            "agent_id": "a",
            "runs": [{"returns": [0.01, 0.02]}, {"returns": [0.03, 0.04]}],
            "run_keys": [{"window": "w0", "seed": 0}],
        }])
        .to_string();
        assert_eq!(
            parse_keyed_field(&json).unwrap_err(),
            RunIdentityError::KeyCountMismatch {
                agent_id: "a".to_string(),
                runs: 2,
                keys: 1,
            }
        );
    }

    #[test]
    fn a_whitespace_padded_window_identity_is_refused() {
        let json = field(&[("a", &[(" w0", 0)])]);
        assert!(matches!(
            parse_keyed_field(&json),
            Err(RunIdentityError::InvalidKey { .. })
        ));
    }

    #[test]
    fn declared_period_identities_must_index_the_returns_and_agree_across_agents() {
        let one = serde_json::json!([{
            "agent_id": "a",
            "runs": [{"returns": [0.01, 0.02]}],
            "run_keys": [{"window": "w0", "seed": 0, "periods": ["d1"]}],
        }])
        .to_string();
        assert_eq!(
            parse_keyed_field(&one).unwrap_err(),
            RunIdentityError::PeriodCountMismatch {
                agent_id: "a".to_string(),
                key: RunKey {
                    window: "w0".into(),
                    seed: 0
                },
                periods: 1,
                returns: 2,
            }
        );

        let two = serde_json::json!([
            {
                "agent_id": "a",
                "runs": [{"returns": [0.01, 0.02]}],
                "run_keys": [{"window": "w0", "seed": 0, "periods": ["d1", "d2"]}],
            },
            {
                "agent_id": "b",
                "runs": [{"returns": [0.03, 0.04]}],
                "run_keys": [{"window": "w0", "seed": 0, "periods": ["d1", "d3"]}],
            },
        ])
        .to_string();
        assert_eq!(
            parse_keyed_field(&two).unwrap_err(),
            RunIdentityError::PeriodMismatch {
                key: RunKey {
                    window: "w0".into(),
                    seed: 0
                },
                agent_id: "b".to_string(),
                other_agent_id: "a".to_string(),
                index: 1,
            }
        );
    }

    fn periods_field(entries: &[(&str, &[&str])]) -> String {
        let docs: Vec<serde_json::Value> = entries
            .iter()
            .map(|(agent_id, periods)| {
                let returns: Vec<f64> = (0..periods.len().max(2))
                    .map(|i| 0.001 * (i as f64 + 1.0))
                    .collect();
                let mut key = serde_json::json!({"window": "w0", "seed": 0});
                if !periods.is_empty() {
                    key["periods"] = serde_json::json!(periods);
                }
                serde_json::json!({
                    "agent_id": agent_id,
                    "runs": [{"returns": returns}],
                    "run_keys": [key],
                })
            })
            .collect();
        serde_json::to_string(&docs).expect("serialize field")
    }

    fn w0() -> RunKey {
        RunKey {
            window: "w0".into(),
            seed: 0,
        }
    }

    #[test]
    fn a_run_repeating_a_period_is_refused_naming_the_period_and_both_indices() {
        // Distinct periods are the control: the same shape parses.
        let distinct = periods_field(&[("a", &["d1", "d2", "d3", "d4"])]);
        assert!(parse_keyed_field(&distinct).is_ok());

        let repeated = periods_field(&[("a", &["d1", "d2", "d3", "d2"])]);
        let error = parse_keyed_field(&repeated).unwrap_err();
        assert_eq!(
            error,
            RunIdentityError::DuplicatePeriod {
                agent_id: "a".to_string(),
                key: w0(),
                period: "d2".to_string(),
                first: 1,
                second: 3,
            }
        );
        assert_eq!(
            error.to_string(),
            "run identity: agent `a` (window `w0`, seed 0) declares period `d2` at \
             index 1 and again at index 3; one period cannot contribute two returns \
             to a run"
        );
    }

    #[test]
    fn the_first_repeat_is_the_one_reported() {
        assert_eq!(first_repeated_period(&[]), None);
        let periods: Vec<String> = ["d1", "d2", "d3", "d3", "d1"]
            .iter()
            .map(|p| p.to_string())
            .collect();
        assert_eq!(first_repeated_period(&periods), Some((2, 3)));
    }

    #[test]
    fn a_repeated_period_is_refused_even_when_every_agent_repeats_it() {
        // Agreement across agents is the cross-agent check, and it passes
        // here. The repeat is still one period counted twice.
        let shared = periods_field(&[("a", &["d1", "d1"]), ("b", &["d1", "d1"])]);
        assert_eq!(
            parse_keyed_field(&shared).unwrap_err(),
            RunIdentityError::DuplicatePeriod {
                agent_id: "a".to_string(),
                key: w0(),
                period: "d1".to_string(),
                first: 0,
                second: 1,
            }
        );

        // With only one agent declaring periods there is nothing to compare
        // against, and the repeat is still refused.
        let one_declares = periods_field(&[("a", &[]), ("b", &["d1", "d1"])]);
        assert_eq!(
            parse_keyed_field(&one_declares).unwrap_err(),
            RunIdentityError::DuplicatePeriod {
                agent_id: "b".to_string(),
                key: w0(),
                period: "d1".to_string(),
                first: 0,
                second: 1,
            }
        );
    }

    /// One run's cell as `(window, seed, periods)`.
    type Cell<'a> = (&'a str, u64, &'a [&'a str]);

    /// A field given as cells per agent. A cell with periods has one return per
    /// period; a cell without has two returns and declares no period axis.
    fn cells_field(entries: &[(&str, &[Cell<'_>])]) -> String {
        let docs: Vec<serde_json::Value> = entries
            .iter()
            .map(|(agent_id, cells)| {
                serde_json::json!({
                    "agent_id": agent_id,
                    "runs": cells
                        .iter()
                        .map(|(_, seed, periods)| serde_json::json!({
                            "returns": (0..periods.len().max(2))
                                .map(|i| 0.001 * (i as f64 + 1.0) + *seed as f64 * 0.0001)
                                .collect::<Vec<_>>(),
                        }))
                        .collect::<Vec<_>>(),
                    "run_keys": cells
                        .iter()
                        .map(|(window, seed, periods)| {
                            let mut key = serde_json::json!({"window": window, "seed": seed});
                            if !periods.is_empty() {
                                key["periods"] = serde_json::json!(periods);
                            }
                            key
                        })
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        serde_json::to_string(&docs).expect("serialize field")
    }

    fn cell(window: &str, seed: u64) -> RunKey {
        RunKey {
            window: window.into(),
            seed,
        }
    }

    fn overlap(
        period: &str,
        first: RunKey,
        first_agent_id: &str,
        second: RunKey,
        second_agent_id: &str,
    ) -> RunIdentityError {
        RunIdentityError::PeriodInTwoWindows(Box::new(PeriodOverlap {
            period: period.to_string(),
            first,
            first_agent_id: first_agent_id.to_string(),
            second,
            second_agent_id: second_agent_id.to_string(),
        }))
    }

    #[test]
    fn seeds_of_one_window_share_its_periods() {
        let both: &[Cell<'_>] = &[
            ("w0", 0, &["a", "b"]),
            ("w0", 1, &["a", "b"]),
            ("w1", 0, &["c", "d"]),
            ("w1", 1, &["c", "d"]),
        ];
        let keyed = parse_keyed_field(&cells_field(&[("x", both), ("y", both)]))
            .expect("replicate seeds of one window are the grid, not a repeat");
        assert_eq!(keyed.seeds, vec![0, 1]);
        assert_eq!(keyed.windows, vec!["w0".to_string(), "w1".to_string()]);
    }

    #[test]
    fn a_period_declared_in_two_windows_is_refused() {
        // w1 restates b, which w0 already covers: the pooled track would hold
        // b's return twice. Both agents declare it, so the cells agree.
        let overlapping: &[Cell<'_>] = &[("w0", 0, &["a", "b"]), ("w1", 0, &["b", "c"])];
        let error =
            parse_keyed_field(&cells_field(&[("x", overlapping), ("y", overlapping)])).unwrap_err();
        assert_eq!(error, overlap("b", cell("w0", 0), "x", cell("w1", 0), "x"));
        assert_eq!(
            error.to_string(),
            "run identity: period `b` is declared in (window `w0`, seed 0) by agent `x` \
             and in (window `w1`, seed 0) by agent `x`; a period belongs to one window, \
             and the pooled track would count its return twice. Seeds of one window may \
             share periods, different windows may not"
        );

        // The same windows over the same periods, the shape of a field that
        // imported one window twice under two labels.
        let copied: &[Cell<'_>] = &[("w0", 0, &["a", "b"]), ("w1", 0, &["a", "b"])];
        assert_eq!(
            parse_keyed_field(&cells_field(&[("x", copied)])).unwrap_err(),
            overlap("a", cell("w0", 0), "x", cell("w1", 0), "x")
        );

        // Disjoint windows are the control.
        let disjoint: &[Cell<'_>] = &[("w0", 0, &["a", "b"]), ("w1", 0, &["c", "d"])];
        assert!(parse_keyed_field(&cells_field(&[("x", disjoint)])).is_ok());
    }

    #[test]
    fn a_period_in_two_windows_is_refused_whatever_the_seeds() {
        // At each seed the two windows are disjoint, but period c sits in w0
        // at seed 1 and in w1 at seed 0.
        let crossed: &[Cell<'_>] = &[
            ("w0", 0, &["a", "b"]),
            ("w0", 1, &["c", "d"]),
            ("w1", 0, &["c", "d"]),
            ("w1", 1, &["a", "b"]),
        ];
        assert_eq!(
            parse_keyed_field(&cells_field(&[("x", crossed)])).unwrap_err(),
            overlap("c", cell("w0", 1), "x", cell("w1", 0), "x")
        );
    }

    #[test]
    fn a_period_in_two_windows_is_refused_when_different_agents_declare_them() {
        // No agent declares both windows, so no single submission repeats a
        // period. The cells still describe the same field: w0 (from x) and w1
        // (from y) both hold period b.
        let x: &[Cell<'_>] = &[("w0", 0, &["a", "b"]), ("w1", 0, &[])];
        let y: &[Cell<'_>] = &[("w0", 0, &[]), ("w1", 0, &["b", "c"])];
        assert_eq!(
            parse_keyed_field(&cells_field(&[("x", x), ("y", y)])).unwrap_err(),
            overlap("b", cell("w0", 0), "x", cell("w1", 0), "y")
        );
    }

    #[test]
    fn a_keyed_field_still_parses_as_a_plain_submission_array() {
        // `run_keys` is a sidecar: existing readers ignore it.
        let json = field(&[("a", &[("w0", 0)])]);
        let subs: Vec<AgentSubmission> = serde_json::from_str(&json).expect("plain parse");
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].runs.len(), 1);
    }

    #[test]
    fn an_empty_field_has_no_grid() {
        assert_eq!(
            parse_keyed_field("[]").unwrap_err(),
            RunIdentityError::EmptyField
        );
    }
}
