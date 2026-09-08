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
//! 5. declared period identities agree across agents for the same cell and
//!    match the run's return length.
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
/// present it must have one entry per return, and two agents on the same cell
/// must declare the same periods.
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
    /// Two agents declared different period identities for the same cell.
    PeriodMismatch {
        key: RunKey,
        agent_id: String,
        other_agent_id: String,
        index: usize,
    },
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
        }
    }
}

impl std::error::Error for RunIdentityError {}

/// A field whose runs are identified rather than positioned: every submission
/// carries the same complete set of cells, and its runs are reordered into the
/// shared canonical `keys` order.
#[derive(Clone, Debug)]
pub struct KeyedField {
    /// The canonical cell order, sorted by `(window, seed)`. Index `i` of every
    /// submission's `runs` is `keys[i]` for every agent.
    pub keys: Vec<RunKey>,
    /// The distinct window identities, sorted.
    pub windows: Vec<String>,
    /// The distinct execution seeds, sorted.
    pub seeds: Vec<u64>,
    /// Submissions in input order, runs reordered into `keys` order.
    pub submissions: Vec<AgentSubmission>,
    /// Mandate declarations, unchanged from [`parse_declared_field`].
    pub declarations: MandateDeclarations,
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

/// Parse a submissions field and require typed run identity throughout.
///
/// Refuses a legacy unkeyed array, a partial grid, a duplicated cell, and
/// disagreeing period identities. On success the returned submissions are
/// reordered so positional access downstream is keyed access.
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

    // Completeness: the union must be the whole window times seed product. No
    // axis is inferred beyond the identities actually submitted.
    let windows: Vec<String> = union
        .iter()
        .map(|key| key.window.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
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

    // Canonical order, applied to every submission.
    let keys: Vec<RunKey> = union.into_iter().collect();
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
        // The two agents list the same four cells in opposite orders. Position
        // alignment would compare different cells; keyed alignment must not.
        let json = field(&[
            ("a", &[("w0", 0), ("w0", 1), ("w1", 0), ("w1", 1)]),
            ("b", &[("w1", 1), ("w1", 0), ("w0", 1), ("w0", 0)]),
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
