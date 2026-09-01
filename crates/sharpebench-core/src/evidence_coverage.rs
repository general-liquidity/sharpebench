//! What the digest actually covers, declared in machine-readable form.
//!
//! A signed evidence record is only as strong as the statement of *what it
//! signs*. The failure mode this module exists to prevent is the quiet one: a
//! digest is computed over most of a record, one field group is left out, and
//! nothing anywhere says so. A consumer holding the document cannot tell the
//! signed fields from the unsigned ones by inspection, so the whole record reads
//! as attested when part of it is not. Nobody has to be dishonest for this to
//! happen; a field added later that nobody remembers to include produces exactly
//! the same result.
//!
//! So coverage is declared, not implied:
//!
//! - Every field of an evidence document carries a [`Coverage`] entry naming the
//!   digest that binds it, or naming the reason it is deliberately unbound.
//! - [`EvidenceInventory::audit`] reports fields present in the document but
//!   missing from the inventory, and fields declared but no longer present. The
//!   schema tests in this module run that audit against the real structs, so a
//!   new field fails the build until somebody decides what it is.
//! - [`EvidenceInventory::preimage`] is the only supported way to build the bytes
//!   that get hashed. It emits covered fields, emits [`REDACTED`] in place of the
//!   value of a secret-bearing field, and refuses a value set that does not match
//!   the declaration.
//!
//! Secrets are redacted **before** hashing rather than omitted, so the field's
//! presence is still bound while its value never enters the preimage. Verifying
//! a document therefore never requires holding the secret.
//!
//! The preimage is a byte string, not a hash: this crate is pure and carries no
//! cryptographic dependency. Feed the output to a digest function such as
//! `sharpebench_attest::content_digest`.

use std::collections::BTreeSet;

use serde::Serialize;

/// The token substituted for a redacted value. Fixed and public, so a verifier
/// reconstructs the preimage without the secret.
pub const REDACTED: &str = "<redacted>";

/// Which digest binds a field.
///
/// Two digests exist because two different things are being attested. An agent's
/// own scored evidence is reproducible from its submission alone; the
/// field-relative figures are not, because they move whenever the field's
/// composition changes. Binding both under one digest would make a re-run with a
/// new entrant invalidate every previously published per-agent attestation.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DigestId {
    /// Fields determined by the agent's own runs and the score configuration.
    AgentScore,
    /// Fields determined by the composition of the field the agent was scored in.
    FieldContext,
    /// Fields describing how a run was produced: dataset, seeds, harness.
    RunProvenance,
}

/// The coverage status of one evidence field.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "coverage", rename_all = "snake_case")]
pub enum Coverage {
    /// The field's value enters the named digest's preimage verbatim.
    Covered { digest: DigestId },
    /// The field is bound by the named digest, but as [`REDACTED`] rather than
    /// as its value, because the value is secret. `reason` states why.
    Redacted {
        digest: DigestId,
        reason: &'static str,
    },
    /// The field enters no digest at all. `reason` states why, and a reason is
    /// mandatory: an exclusion nobody can justify is a bug, not a policy.
    Excluded { reason: &'static str },
}

impl Coverage {
    /// The digest binding this field, if any.
    pub fn digest(&self) -> Option<DigestId> {
        match self {
            Coverage::Covered { digest } | Coverage::Redacted { digest, .. } => Some(*digest),
            Coverage::Excluded { .. } => None,
        }
    }

    /// Whether the field's value (as opposed to its presence) is bound.
    pub fn binds_value(&self) -> bool {
        matches!(self, Coverage::Covered { .. })
    }
}

/// A declaration of what is and is not covered, for one evidence document.
#[derive(Clone, Copy, Debug)]
pub struct EvidenceInventory {
    /// The document this inventory describes, by type name.
    pub document: &'static str,
    /// Every field of the document, in serialization order.
    pub fields: &'static [(&'static str, Coverage)],
}

/// What an inventory audit found. Empty on both counts is the passing state.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct InventoryAudit {
    /// Fields present in the document but absent from the inventory. Each of
    /// these is a field a consumer cannot classify.
    pub undeclared: Vec<String>,
    /// Fields declared by the inventory but no longer present in the document.
    pub stale: Vec<String>,
}

impl InventoryAudit {
    /// Whether the inventory matches the document exactly.
    pub fn is_complete(&self) -> bool {
        self.undeclared.is_empty() && self.stale.is_empty()
    }
}

/// A value set that does not match the inventory.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum PreimageError {
    /// A value was supplied for a field the inventory does not declare.
    UndeclaredField { field: String },
    /// A field bound by the requested digest had no value supplied.
    MissingField { field: String },
    /// The same field was supplied more than once.
    DuplicateField { field: String },
}

impl EvidenceInventory {
    /// The coverage declared for `field`, or `None` if it is undeclared.
    pub fn coverage(&self, field: &str) -> Option<Coverage> {
        self.fields
            .iter()
            .find(|(name, _)| *name == field)
            .map(|(_, c)| *c)
    }

    /// Field names bound by `digest`, in serialization order. Includes redacted
    /// fields, whose presence is bound even though their value is not.
    pub fn fields_for(&self, digest: DigestId) -> Vec<&'static str> {
        self.fields
            .iter()
            .filter(|(_, c)| c.digest() == Some(digest))
            .map(|(name, _)| *name)
            .collect()
    }

    /// Field names declared as covered by no digest, with their stated reasons.
    pub fn exclusions(&self) -> Vec<(&'static str, &'static str)> {
        self.fields
            .iter()
            .filter_map(|(name, c)| match c {
                Coverage::Excluded { reason } => Some((*name, *reason)),
                Coverage::Covered { .. } | Coverage::Redacted { .. } => None,
            })
            .collect()
    }

    /// Compare the inventory against the field names a document actually has.
    ///
    /// This is what keeps the declaration from drifting: run it in a test with
    /// the serialized field names of the real struct.
    pub fn audit<S: AsRef<str>>(&self, observed: &[S]) -> InventoryAudit {
        let declared: BTreeSet<&str> = self.fields.iter().map(|(n, _)| *n).collect();
        let present: BTreeSet<&str> = observed.iter().map(|s| s.as_ref()).collect();
        InventoryAudit {
            undeclared: present
                .difference(&declared)
                .map(|s| (*s).to_string())
                .collect(),
            stale: declared
                .difference(&present)
                .map(|s| (*s).to_string())
                .collect(),
        }
    }

    /// Build the canonical bytes to hash for one digest.
    ///
    /// `values` supplies every field of the document as an already-rendered
    /// string; the caller owns the rendering so that number formatting stays a
    /// decision of the producer rather than of this module. Fields bound by
    /// other digests, and excluded fields, are simply not consulted.
    ///
    /// The output is `name\x1fvalue\x1e` per field, in inventory order, with the
    /// document name and digest as the first record. The unit separators cannot
    /// appear in a field name, so no value can be crafted to look like a field
    /// boundary.
    ///
    /// Fails when a value is supplied for an undeclared field (the inventory is
    /// out of date), when a bound field has no value (the digest would silently
    /// cover less than it claims), or when a field is supplied twice.
    pub fn preimage(
        &self,
        digest: DigestId,
        values: &[(&str, &str)],
    ) -> Result<Vec<u8>, PreimageError> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for (name, _) in values {
            if self.coverage(name).is_none() {
                return Err(PreimageError::UndeclaredField {
                    field: (*name).to_string(),
                });
            }
            if !seen.insert(name) {
                return Err(PreimageError::DuplicateField {
                    field: (*name).to_string(),
                });
            }
        }

        let mut out = Vec::new();
        push_record(&mut out, "sharpebench.document", self.document);
        push_record(&mut out, "sharpebench.digest", digest_name(digest));

        for (name, coverage) in self.fields {
            if coverage.digest() != Some(digest) {
                continue;
            }
            let Some((_, value)) = values.iter().find(|(n, _)| n == name) else {
                return Err(PreimageError::MissingField {
                    field: (*name).to_string(),
                });
            };
            match coverage {
                Coverage::Covered { .. } => push_record(&mut out, name, value),
                Coverage::Redacted { .. } => push_record(&mut out, name, REDACTED),
                Coverage::Excluded { .. } => unreachable!("filtered by digest match"),
            }
        }
        Ok(out)
    }
}

fn push_record(out: &mut Vec<u8>, name: &str, value: &str) {
    out.extend_from_slice(name.as_bytes());
    out.push(0x1f);
    out.extend_from_slice(value.as_bytes());
    out.push(0x1e);
}

fn digest_name(d: DigestId) -> &'static str {
    match d {
        DigestId::AgentScore => "agent_score",
        DigestId::FieldContext => "field_context",
        DigestId::RunProvenance => "run_provenance",
    }
}

const COVERED_SCORE: Coverage = Coverage::Covered {
    digest: DigestId::AgentScore,
};
const COVERED_FIELD: Coverage = Coverage::Covered {
    digest: DigestId::FieldContext,
};

/// Coverage declaration for [`crate::composite::CompositeScore`], the per-agent
/// evidence record the leaderboard publishes.
///
/// Split by what determines the value. `AgentScore` fields come out of the
/// agent's own runs and the score configuration; `FieldContext` fields come out
/// of the composition of the field it was scored against and change when another
/// entrant is added. Three fields are bound by neither, each with its reason.
pub const COMPOSITE_SCORE_INVENTORY: EvidenceInventory = EvidenceInventory {
    document: "sharpebench_core::composite::CompositeScore",
    fields: &[
        ("agent_id", COVERED_SCORE),
        ("deflated_sharpe", COVERED_SCORE),
        ("psr", COVERED_SCORE),
        ("passed_k", COVERED_SCORE),
        ("process_ok", COVERED_SCORE),
        ("bootstrap_p", COVERED_SCORE),
        ("raw_mean_return", COVERED_SCORE),
        ("rank_eligible", COVERED_SCORE),
        ("composite", COVERED_SCORE),
        ("alpha", COVERED_FIELD),
        ("beta", COVERED_FIELD),
        ("calibration_brier", COVERED_SCORE),
        ("calibration_observations", COVERED_SCORE),
        ("edge_half_life", COVERED_SCORE),
        ("field_reality_check_p", COVERED_FIELD),
        ("max_drawdown", COVERED_SCORE),
        ("mandate_ok", COVERED_SCORE),
        ("worst_run_drawdown", COVERED_SCORE),
        ("turnover", COVERED_SCORE),
        ("pareto_optimal", COVERED_FIELD),
        ("step_down_significant", COVERED_FIELD),
        ("confidence_weighted_return", COVERED_SCORE),
        ("cost", COVERED_SCORE),
        ("return_per_cost", COVERED_SCORE),
        ("field_spa_p", COVERED_FIELD),
        ("field_spa_consistent_p", COVERED_FIELD),
        ("field_significance_benchmark", COVERED_FIELD),
        ("field_crowdedness", COVERED_FIELD),
        ("field_crowdedness_peers", COVERED_FIELD),
        ("in_sample_trials", COVERED_SCORE),
        ("effective_n_trials", COVERED_SCORE),
        ("dsr_percentile", COVERED_FIELD),
        ("selection_median_dsr", COVERED_SCORE),
        ("selection_gap", COVERED_SCORE),
        ("rank_ordinal", COVERED_FIELD),
        ("rolling_min_sharpe", COVERED_SCORE),
        ("rolling_frac_positive", COVERED_SCORE),
        ("rolling_windows", COVERED_SCORE),
        ("sortino", COVERED_SCORE),
        ("downside_deviation", COVERED_SCORE),
        ("dsr_per_cost", COVERED_SCORE),
        ("process_floored", COVERED_SCORE),
        ("realized_floored_return", COVERED_SCORE),
        ("dsr_ci_low", COVERED_SCORE),
        ("dsr_ci_high", COVERED_SCORE),
        ("dsr_se", COVERED_SCORE),
        ("tie_group", COVERED_FIELD),
        ("dsr_tied", COVERED_FIELD),
        ("trials_sr_std", COVERED_SCORE),
        ("trials_sr_std_annualized", COVERED_SCORE),
        (
            "trials_sr_std_annualized_equivalent",
            Coverage::Excluded {
                reason: "presentation unit of trials_sr_std, which is covered by agent_score; \
                         a fixed conversion carries no evidence the covered value does not, \
                         and binding it would let a units edit break a digest over unchanged \
                         evidence",
            },
        ),
        ("deflation_bar_per_period", COVERED_SCORE),
        (
            "deflation_bar_annualized_equivalent",
            Coverage::Excluded {
                reason: "presentation unit of deflation_bar_per_period, which is covered by \
                         agent_score; excluded for the reason given on \
                         trials_sr_std_annualized_equivalent",
            },
        ),
        ("deflation_null_mean_per_period", COVERED_SCORE),
        ("pooled_observations", COVERED_SCORE),
        ("trials_sr_std_source", COVERED_SCORE),
        ("runs_submitted", COVERED_SCORE),
        ("runs_scored", COVERED_SCORE),
        ("process_score", COVERED_SCORE),
        ("process_warnings", COVERED_SCORE),
        ("econ_rationality_score", COVERED_SCORE),
        ("econ_dominance_violations", COVERED_SCORE),
        (
            "role_contributions",
            Coverage::Excluded {
                reason: "a variable-length nested structure whose element type carries its own \
                         fields; binding it through this flat inventory would go stale silently \
                         the moment RoleContribution gains a field, which is the exact failure \
                         this inventory exists to prevent. It needs its own inventory before it \
                         can be bound",
            },
        ),
        ("declared_mandate", COVERED_SCORE),
        ("verdict_applied", COVERED_SCORE),
        ("declared_passed_k", COVERED_SCORE),
        ("declared_mandate_eligible", COVERED_FIELD),
        ("declared_mandate_ordinal", COVERED_FIELD),
    ],
};

/// How one scored run was produced.
///
/// Distinct from the score itself: two identical scores produced from different
/// datasets, seeds or harness builds are not the same evidence, and a digest
/// over the score alone cannot tell them apart.
///
/// `dataset_canary` is the leak tripwire bound into a held-out set. It is the
/// one field here that must never be recoverable from a published preimage, and
/// it is declared [`Coverage::Redacted`] for that reason.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RunProvenance {
    pub agent_id: String,
    pub dataset_id: String,
    /// Content hash of the frozen dataset bytes.
    pub dataset_content_hash: String,
    /// The canary GUID embedded in the held-out set. Secret.
    pub dataset_canary: String,
    pub window_id: String,
    pub seed: u64,
    pub cost_profile: String,
    pub harness_version: String,
}

/// Coverage declaration for [`RunProvenance`].
pub const RUN_PROVENANCE_INVENTORY: EvidenceInventory = EvidenceInventory {
    document: "sharpebench_core::evidence_coverage::RunProvenance",
    fields: &[
        (
            "agent_id",
            Coverage::Covered {
                digest: DigestId::RunProvenance,
            },
        ),
        (
            "dataset_id",
            Coverage::Covered {
                digest: DigestId::RunProvenance,
            },
        ),
        (
            "dataset_content_hash",
            Coverage::Covered {
                digest: DigestId::RunProvenance,
            },
        ),
        (
            "dataset_canary",
            Coverage::Redacted {
                digest: DigestId::RunProvenance,
                reason: "the canary is a leak tripwire: publishing a preimage that contains it \
                         hands an agent the token whose appearance in training data is the \
                         evidence of contamination. Binding its presence is enough, and \
                         verification must not require holding it",
            },
        ),
        (
            "window_id",
            Coverage::Covered {
                digest: DigestId::RunProvenance,
            },
        ),
        (
            "seed",
            Coverage::Covered {
                digest: DigestId::RunProvenance,
            },
        ),
        (
            "cost_profile",
            Coverage::Covered {
                digest: DigestId::RunProvenance,
            },
        ),
        (
            "harness_version",
            Coverage::Covered {
                digest: DigestId::RunProvenance,
            },
        ),
    ],
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::{
        rank_declared, AgentSubmission, DeclaredMandate, MandateDeclarations, Run, ScoreConfig,
    };
    use crate::process::Trace;

    /// Top-level field names of a serialized value.
    fn field_names<T: Serialize>(v: &T) -> Vec<String> {
        let json = serde_json::to_value(v).expect("value serializes");
        json.as_object()
            .expect("evidence documents serialize as objects")
            .keys()
            .cloned()
            .collect()
    }

    fn probe_submission(id: &str, mean: f64) -> AgentSubmission {
        let runs: Vec<Run> = (0..5)
            .map(|_| Run {
                returns: (0..60)
                    .map(|i| mean + 0.0005 * (i as f64 * 0.7).sin())
                    .collect(),
                trace: Trace::default(),
                confidences: Vec::new(),
                outcomes: Vec::new(),
                cost: 1.0,
            })
            .collect();
        AgentSubmission {
            agent_id: id.to_string(),
            runs,
            in_sample_trials: 0,
            candidates: Vec::new(),
        }
    }

    /// Every field name a `CompositeScore` can carry.
    ///
    /// Five of the declared-mandate fields are `skip_serializing_if =
    /// "Option::is_none"`, so one probe never shows the whole schema. The union
    /// over a declared and an undeclared probe does, and taking the union is
    /// what makes the audit total rather than a sample.
    fn observed_composite_score_fields() -> Vec<String> {
        let subs = vec![
            probe_submission("declared-probe", 0.002),
            probe_submission("undeclared-probe", 0.0018),
        ];
        let mut declarations = MandateDeclarations::new();
        declarations.insert(
            "declared-probe".to_string(),
            DeclaredMandate::AbsoluteReturn,
        );
        let scored = rank_declared(&subs, &declarations, &ScoreConfig::default());

        let mut names: BTreeSet<String> = BTreeSet::new();
        for s in &scored {
            names.extend(field_names(s));
        }
        names.into_iter().collect()
    }

    // -----------------------------------------------------------------------
    // The drift guards. These are the tests that fail on a new field.
    // -----------------------------------------------------------------------

    #[test]
    fn every_composite_score_field_is_declared_covered_or_excluded() {
        let names = observed_composite_score_fields();
        let audit = COMPOSITE_SCORE_INVENTORY.audit(&names);
        assert!(
            audit.is_complete(),
            "CompositeScore and its coverage inventory have diverged.\n\
             Fields present but undeclared: {:?}\n\
             Fields declared but absent: {:?}\n\
             Add each new field to COMPOSITE_SCORE_INVENTORY as Covered {{ digest }} \
             or Excluded {{ reason }}.",
            audit.undeclared,
            audit.stale
        );
    }

    #[test]
    fn every_run_provenance_field_is_declared_covered_or_excluded() {
        let p = RunProvenance {
            agent_id: "a".to_string(),
            dataset_id: "us-indices-1d".to_string(),
            dataset_content_hash: "abc".to_string(),
            dataset_canary: "canary-token".to_string(),
            window_id: "w3".to_string(),
            seed: 7,
            cost_profile: "retail".to_string(),
            harness_version: "0.11.0".to_string(),
        };
        let audit = RUN_PROVENANCE_INVENTORY.audit(&field_names(&p));
        assert!(audit.is_complete(), "{audit:?}");
    }

    #[test]
    fn a_new_undeclared_field_fails_the_audit() {
        // The guard above passes only because the inventory is current. Prove it
        // can actually fail: hand it a document with one extra field.
        let mut names = observed_composite_score_fields();
        names.push("newly_added_metric".to_string());
        let audit = COMPOSITE_SCORE_INVENTORY.audit(&names);
        assert!(!audit.is_complete());
        assert_eq!(audit.undeclared, vec!["newly_added_metric".to_string()]);
        assert!(audit.stale.is_empty());
    }

    #[test]
    fn a_removed_field_is_reported_as_stale() {
        let names: Vec<String> = observed_composite_score_fields()
            .into_iter()
            .filter(|n| n != "psr")
            .collect();
        let audit = COMPOSITE_SCORE_INVENTORY.audit(&names);
        assert_eq!(audit.stale, vec!["psr".to_string()]);
    }

    // -----------------------------------------------------------------------
    // The declaration itself.
    // -----------------------------------------------------------------------

    #[test]
    fn every_exclusion_carries_a_reason() {
        for inventory in [COMPOSITE_SCORE_INVENTORY, RUN_PROVENANCE_INVENTORY] {
            for (name, reason) in inventory.exclusions() {
                assert!(
                    reason.len() > 30,
                    "{}::{name} is excluded without a usable reason",
                    inventory.document
                );
            }
        }
    }

    #[test]
    fn every_redaction_carries_a_reason() {
        for inventory in [COMPOSITE_SCORE_INVENTORY, RUN_PROVENANCE_INVENTORY] {
            for (name, coverage) in inventory.fields {
                if let Coverage::Redacted { reason, .. } = coverage {
                    assert!(
                        reason.len() > 30,
                        "{}::{name} is redacted without a usable reason",
                        inventory.document
                    );
                }
            }
        }
    }

    #[test]
    fn the_two_digests_partition_the_covered_fields() {
        let score = COMPOSITE_SCORE_INVENTORY.fields_for(DigestId::AgentScore);
        let context = COMPOSITE_SCORE_INVENTORY.fields_for(DigestId::FieldContext);
        let excluded = COMPOSITE_SCORE_INVENTORY.exclusions();
        assert_eq!(
            score.len() + context.len() + excluded.len(),
            COMPOSITE_SCORE_INVENTORY.fields.len(),
            "every field lands in exactly one of the three buckets"
        );
        for f in &score {
            assert!(!context.contains(f), "{f} is claimed by both digests");
        }
        assert_eq!(excluded.len(), 3, "exactly three deliberate exclusions");
    }

    #[test]
    fn no_field_is_declared_twice() {
        for inventory in [COMPOSITE_SCORE_INVENTORY, RUN_PROVENANCE_INVENTORY] {
            let names: BTreeSet<&str> = inventory.fields.iter().map(|(n, _)| *n).collect();
            assert_eq!(
                names.len(),
                inventory.fields.len(),
                "{} declares a field twice",
                inventory.document
            );
        }
    }

    #[test]
    fn coverage_lookup_answers_the_consumer_question() {
        assert_eq!(
            COMPOSITE_SCORE_INVENTORY.coverage("deflated_sharpe"),
            Some(COVERED_SCORE)
        );
        assert_eq!(
            COMPOSITE_SCORE_INVENTORY
                .coverage("alpha")
                .unwrap()
                .digest(),
            Some(DigestId::FieldContext)
        );
        assert!(!COMPOSITE_SCORE_INVENTORY
            .coverage("role_contributions")
            .unwrap()
            .binds_value());
        assert_eq!(COMPOSITE_SCORE_INVENTORY.coverage("not_a_field"), None);
    }

    // -----------------------------------------------------------------------
    // Preimage construction and redaction.
    // -----------------------------------------------------------------------

    fn provenance_values() -> Vec<(&'static str, &'static str)> {
        vec![
            ("agent_id", "donchian-20-10"),
            ("dataset_id", "us-indices-1d"),
            ("dataset_content_hash", "9f86d081884c7d65"),
            ("dataset_canary", "SUPER-SECRET-CANARY-42"),
            ("window_id", "w3"),
            ("seed", "7"),
            ("cost_profile", "retail"),
            ("harness_version", "0.11.0"),
        ]
    }

    #[test]
    fn a_secret_never_reaches_the_preimage() {
        let bytes = RUN_PROVENANCE_INVENTORY
            .preimage(DigestId::RunProvenance, &provenance_values())
            .expect("complete value set");
        let text = String::from_utf8(bytes).unwrap();
        assert!(
            !text.contains("SUPER-SECRET-CANARY-42"),
            "the canary leaked into the bytes that get hashed"
        );
        assert!(
            text.contains(REDACTED),
            "the field's presence must still be bound"
        );
        assert!(text.contains("dataset_content_hash"));
    }

    #[test]
    fn changing_a_secret_does_not_change_the_preimage() {
        // The consequence of redacting rather than hashing the value: a verifier
        // who does not hold the canary still reproduces the digest exactly.
        let base = RUN_PROVENANCE_INVENTORY
            .preimage(DigestId::RunProvenance, &provenance_values())
            .unwrap();
        let mut rotated = provenance_values();
        rotated[3] = ("dataset_canary", "A-DIFFERENT-CANARY");
        let after = RUN_PROVENANCE_INVENTORY
            .preimage(DigestId::RunProvenance, &rotated)
            .unwrap();
        assert_eq!(base, after);
    }

    #[test]
    fn changing_a_covered_value_changes_the_preimage() {
        let base = RUN_PROVENANCE_INVENTORY
            .preimage(DigestId::RunProvenance, &provenance_values())
            .unwrap();
        let mut tampered = provenance_values();
        tampered[2] = ("dataset_content_hash", "0000000000000000");
        let after = RUN_PROVENANCE_INVENTORY
            .preimage(DigestId::RunProvenance, &tampered)
            .unwrap();
        assert_ne!(base, after);
    }

    #[test]
    fn the_preimage_is_deterministic_and_order_independent() {
        let mut shuffled = provenance_values();
        shuffled.reverse();
        assert_eq!(
            RUN_PROVENANCE_INVENTORY
                .preimage(DigestId::RunProvenance, &provenance_values())
                .unwrap(),
            RUN_PROVENANCE_INVENTORY
                .preimage(DigestId::RunProvenance, &shuffled)
                .unwrap(),
            "inventory order fixes the layout, not caller order"
        );
    }

    #[test]
    fn a_missing_bound_field_is_refused() {
        let mut incomplete = provenance_values();
        incomplete.retain(|(n, _)| *n != "seed");
        assert_eq!(
            RUN_PROVENANCE_INVENTORY.preimage(DigestId::RunProvenance, &incomplete),
            Err(PreimageError::MissingField {
                field: "seed".to_string()
            })
        );
    }

    #[test]
    fn an_undeclared_or_duplicated_value_is_refused() {
        let mut extra = provenance_values();
        extra.push(("newly_added_metric", "1.0"));
        assert_eq!(
            RUN_PROVENANCE_INVENTORY.preimage(DigestId::RunProvenance, &extra),
            Err(PreimageError::UndeclaredField {
                field: "newly_added_metric".to_string()
            })
        );

        let mut dup = provenance_values();
        dup.push(("seed", "8"));
        assert_eq!(
            RUN_PROVENANCE_INVENTORY.preimage(DigestId::RunProvenance, &dup),
            Err(PreimageError::DuplicateField {
                field: "seed".to_string()
            })
        );
    }

    #[test]
    fn an_excluded_field_never_appears_under_any_digest() {
        let values: Vec<(&str, &str)> = COMPOSITE_SCORE_INVENTORY
            .fields
            .iter()
            .map(|(n, _)| (*n, "0"))
            .collect();
        for digest in [DigestId::AgentScore, DigestId::FieldContext] {
            let bytes = COMPOSITE_SCORE_INVENTORY.preimage(digest, &values).unwrap();
            let text = String::from_utf8(bytes).unwrap();
            for (name, _) in COMPOSITE_SCORE_INVENTORY.exclusions() {
                assert!(
                    !text.contains(name),
                    "{name} is excluded but reached the {digest:?} preimage"
                );
            }
        }
    }

    #[test]
    fn the_two_digests_produce_different_preimages_over_the_same_values() {
        let values: Vec<(&str, &str)> = COMPOSITE_SCORE_INVENTORY
            .fields
            .iter()
            .map(|(n, _)| (*n, "0"))
            .collect();
        assert_ne!(
            COMPOSITE_SCORE_INVENTORY
                .preimage(DigestId::AgentScore, &values)
                .unwrap(),
            COMPOSITE_SCORE_INVENTORY
                .preimage(DigestId::FieldContext, &values)
                .unwrap(),
            "the digest tag and the field set both differ"
        );
    }
}
