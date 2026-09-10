//! What an entrant or developer is allowed to read back, declared as an
//! allowlist and enforced by one named seal.
//!
//! [`crate::evidence_coverage`] answers a different question about the same
//! records: which digest *binds* a field. This module answers which fields
//! *leave the host*. The two are independent axes (a field can be signed and
//! withheld, or unsigned and shown), so they are kept as sibling declarations
//! rather than merged into one inventory. They share the drift discipline: a
//! field nobody declared fails a test.
//!
//! The failure this prevents is quiet. A report that echoes a record wholesale
//! leaks whatever that record grows next: a later field carrying held-out window
//! dates, per-run returns or scenario detail reaches every consumer the moment
//! it is added, because nobody decided it should not. A denylist fails open in
//! exactly that case. An allowlist fails closed: [`seal`] emits only declared
//! fields, withholds everything else, and records what it withheld in a
//! [`SealReport`] so a test can see it.
//!
//! The seal preserves the serializer's field order and every number's bytes, so
//! sealing a record whose fields are all declared reproduces its unsealed
//! serialization exactly. The surfaces that return a board row (the CLI JSON
//! board, the WASM and npm `score` calls, the MCP `score` tools, the Python
//! `rank_board` family) therefore emit byte-identical output until a field is
//! added and left undeclared, at which point that field alone disappears.

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

use crate::composite::CompositeScore;
use crate::evidence_coverage::InventoryAudit;

/// Whether one field of a record reaches the reader.
#[derive(Clone, Copy, Debug)]
pub enum Visibility {
    /// The value is shown as serialized. Only a scalar, a string, `null`, or an
    /// array of those qualifies: a value that turns out to contain an object is
    /// withheld, because its inner fields were never reviewed.
    Visible,
    /// The value is an object, or an array of objects, whose own fields are
    /// sealed by the nested allowlist. Scalars and `null` pass through.
    Nested(&'static VisibilityAllowlist),
    /// Deliberately never shown. `reason` states why.
    Withheld { reason: &'static str },
}

/// The declared visibility of every field of one record type.
#[derive(Clone, Copy, Debug)]
pub struct VisibilityAllowlist {
    /// The record this allowlist describes, by type name.
    pub document: &'static str,
    /// Every field of the record, in serialization order.
    pub fields: &'static [(&'static str, Visibility)],
}

impl VisibilityAllowlist {
    /// The visibility declared for `field`, or `None` if it is undeclared.
    pub fn visibility(&self, field: &str) -> Option<Visibility> {
        self.fields
            .iter()
            .find(|(name, _)| *name == field)
            .map(|(_, visibility)| *visibility)
    }

    /// Compare the allowlist against the field names a record actually has.
    /// Run it in a test against the real type so a new field fails the build
    /// until somebody declares it visible or withheld.
    pub fn audit<S: AsRef<str>>(&self, observed: &[S]) -> InventoryAudit {
        let declared: std::collections::BTreeSet<&str> =
            self.fields.iter().map(|(name, _)| *name).collect();
        let present: std::collections::BTreeSet<&str> =
            observed.iter().map(|name| name.as_ref()).collect();
        InventoryAudit {
            undeclared: present
                .difference(&declared)
                .map(|name| (*name).to_string())
                .collect(),
            stale: declared
                .difference(&present)
                .map(|name| (*name).to_string())
                .collect(),
        }
    }
}

/// What a seal removed, by dotted path (`row.field`, `role_contributions.role`).
/// Empty on every count is the state a surface ships in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SealReport {
    /// Fields the allowlist does not mention. Each is a field nobody reviewed.
    pub undeclared: Vec<String>,
    /// Fields declared [`Visibility::Withheld`].
    pub withheld: Vec<String>,
    /// Fields declared [`Visibility::Visible`] whose value now contains an
    /// object: a scalar that became a structure carries fields nobody reviewed.
    pub restructured: Vec<String>,
}

impl SealReport {
    /// Whether the seal removed anything at all.
    pub fn is_empty(&self) -> bool {
        self.undeclared.is_empty() && self.withheld.is_empty() && self.restructured.is_empty()
    }
}

/// A sealed record, ready to serialize. It serializes to exactly the declared
/// fields of the original, in the original order, with the original bytes for
/// every value.
#[derive(Clone, Debug, PartialEq)]
pub struct EntrantView {
    node: Node,
    report: SealReport,
}

impl EntrantView {
    /// What the seal removed.
    pub fn report(&self) -> &SealReport {
        &self.report
    }
}

impl Serialize for EntrantView {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.node.serialize(serializer)
    }
}

/// The seal. Serialize `value`, then keep only what `allowlist` declares. A
/// top-level array is sealed element by element, so a board and a single row
/// go through the same function.
pub fn seal<T: Serialize + ?Sized>(
    value: &T,
    allowlist: &'static VisibilityAllowlist,
) -> Result<EntrantView, serde_json::Error> {
    // The round trip through text is exact: this crate enables serde_json's
    // `float_roundtrip`, and ryu's shortest form is unique per bit pattern, so
    // every float re-serializes to the bytes it was parsed from.
    let text = serde_json::to_string(value)?;
    let node: Node = serde_json::from_str(&text)?;
    let mut report = SealReport::default();
    let node = project(node, allowlist, "", &mut report);
    Ok(EntrantView { node, report })
}

/// Seal a ranked board for any reader outside the host.
pub fn seal_board(board: &[CompositeScore]) -> EntrantView {
    seal(board, &COMPOSITE_SCORE_VISIBILITY).expect("composite scores serialize")
}

/// Seal one scored row for any reader outside the host.
pub fn seal_score(score: &CompositeScore) -> EntrantView {
    seal(score, &COMPOSITE_SCORE_VISIBILITY).expect("composite scores serialize")
}

fn project(
    node: Node,
    allowlist: &'static VisibilityAllowlist,
    prefix: &str,
    report: &mut SealReport,
) -> Node {
    match node {
        Node::Object(entries) => Node::Object(
            entries
                .into_iter()
                .filter_map(|(key, value)| {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    let kept = match allowlist.visibility(&key) {
                        None => {
                            report.undeclared.push(path);
                            None
                        }
                        Some(Visibility::Withheld { .. }) => {
                            report.withheld.push(path);
                            None
                        }
                        Some(Visibility::Visible) if value.contains_object() => {
                            report.restructured.push(path);
                            None
                        }
                        Some(Visibility::Visible) => Some(value),
                        Some(Visibility::Nested(inner)) => {
                            Some(project(value, inner, &path, report))
                        }
                    };
                    kept.map(|value| (key, value))
                })
                .collect(),
        ),
        Node::Array(items) => Node::Array(
            items
                .into_iter()
                .map(|item| project(item, allowlist, prefix, report))
                .collect(),
        ),
        scalar => scalar,
    }
}

/// Order-preserving JSON. `serde_json::Value` sorts object keys unless the
/// `preserve_order` feature is on workspace-wide, which would reorder every
/// existing output; this keeps the serializer's order instead.
#[derive(Clone, Debug, PartialEq)]
enum Node {
    Null,
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    String(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

impl Node {
    fn contains_object(&self) -> bool {
        match self {
            Node::Object(_) => true,
            Node::Array(items) => items.iter().any(Node::contains_object),
            _ => false,
        }
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Node::Null => serializer.serialize_unit(),
            Node::Bool(value) => serializer.serialize_bool(*value),
            Node::U64(value) => serializer.serialize_u64(*value),
            Node::I64(value) => serializer.serialize_i64(*value),
            Node::F64(value) => serializer.serialize_f64(*value),
            Node::String(value) => serializer.serialize_str(value),
            Node::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Node::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NodeVisitor)
    }
}

struct NodeVisitor;

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Node, E> {
        Ok(Node::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Node, E> {
        Ok(Node::I64(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Node, E> {
        Ok(Node::U64(value))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Node, E> {
        Ok(Node::F64(value))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Node, E> {
        Ok(Node::String(value.to_string()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Node, E> {
        Ok(Node::String(value))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Node, D::Error> {
        Node::deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Node::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut entries = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, Node>()? {
            entries.push((key, value));
        }
        Ok(Node::Object(entries))
    }
}

const V: Visibility = Visibility::Visible;

/// Both mandate shapes a row can carry, the submitter's declaration and the
/// verdict the kernel resolved it to. Internally tagged by `kind`.
pub const MANDATE_VISIBILITY: VisibilityAllowlist = VisibilityAllowlist {
    document: "sharpebench_core::composite::{DeclaredMandate, MandateVerdict}",
    fields: &[
        ("kind", V),
        ("benchmark_id", V),
        ("max_per_run_drawdown", V),
    ],
};

/// [`crate::roles::RoleContribution`].
pub const ROLE_CONTRIBUTION_VISIBILITY: VisibilityAllowlist = VisibilityAllowlist {
    document: "sharpebench_core::roles::RoleContribution",
    fields: &[
        ("role", V),
        ("beta_to_team", V),
        ("mean_return", V),
        ("periods", V),
    ],
};

/// [`crate::certification::CertificationGap`], internally tagged by `property`.
pub const CERTIFICATION_GAP_VISIBILITY: VisibilityAllowlist = VisibilityAllowlist {
    document: "sharpebench_core::certification::CertificationGap",
    fields: &[("property", V), ("run", V), ("block_violations", V)],
};

/// [`crate::certification::Certification`].
pub const CERTIFICATION_VISIBILITY: VisibilityAllowlist = VisibilityAllowlist {
    document: "sharpebench_core::certification::Certification",
    fields: &[
        ("mode", V),
        ("certified", V),
        ("lifecycle_warnings", V),
        (
            "withheld",
            Visibility::Nested(&CERTIFICATION_GAP_VISIBILITY),
        ),
    ],
};

/// The board row every surface returns. Every field present when this seal
/// landed is visible, so the existing outputs are unchanged; what the seal adds
/// is that the next field is not.
pub const COMPOSITE_SCORE_VISIBILITY: VisibilityAllowlist = VisibilityAllowlist {
    document: "sharpebench_core::composite::CompositeScore",
    fields: &[
        ("agent_id", V),
        ("deflated_sharpe", V),
        ("psr", V),
        ("passed_k", V),
        ("process_ok", V),
        ("bootstrap_p", V),
        ("bootstrap_error", V),
        ("deflation_error", V),
        ("selection_error", V),
        ("raw_mean_return", V),
        ("rank_eligible", V),
        ("composite", V),
        ("alpha", V),
        ("beta", V),
        ("calibration_brier", V),
        ("calibration_observations", V),
        ("edge_half_life", V),
        ("field_reality_check_p", V),
        ("max_drawdown", V),
        ("mandate_ok", V),
        ("worst_run_drawdown", V),
        ("turnover", V),
        ("pareto_optimal", V),
        ("step_down_significant", V),
        ("confidence_weighted_return", V),
        ("cost", V),
        ("return_per_cost", V),
        ("field_spa_p", V),
        ("field_spa_consistent_p", V),
        ("field_significance_benchmark", V),
        ("field_crowdedness", V),
        ("field_crowdedness_peers", V),
        ("in_sample_trials", V),
        ("effective_n_trials", V),
        ("dsr_percentile", V),
        ("selection_median_dsr", V),
        ("selection_gap", V),
        ("rank_ordinal", V),
        ("rolling_min_sharpe", V),
        ("rolling_frac_positive", V),
        ("rolling_windows", V),
        ("sortino", V),
        ("downside_deviation", V),
        ("dsr_per_cost", V),
        ("process_floored", V),
        ("realized_floored_return", V),
        ("dsr_ci_low", V),
        ("dsr_ci_high", V),
        ("dsr_se", V),
        ("tie_group", V),
        ("dsr_tied", V),
        ("trials_sr_std", V),
        ("trials_sr_std_annualized", V),
        ("trials_sr_std_annualized_equivalent", V),
        ("deflation_bar_per_period", V),
        ("deflation_bar_annualized_equivalent", V),
        ("deflation_null_mean_per_period", V),
        ("pooled_observations", V),
        ("trials_sr_std_source", V),
        ("runs_submitted", V),
        ("runs_scored", V),
        ("process_score", V),
        ("process_warnings", V),
        ("econ_rationality_score", V),
        ("econ_dominance_violations", V),
        (
            "role_contributions",
            Visibility::Nested(&ROLE_CONTRIBUTION_VISIBILITY),
        ),
        ("declared_mandate", Visibility::Nested(&MANDATE_VISIBILITY)),
        ("verdict_applied", Visibility::Nested(&MANDATE_VISIBILITY)),
        ("declared_passed_k", V),
        ("declared_mandate_eligible", V),
        ("declared_mandate_ordinal", V),
        (
            "certification",
            Visibility::Nested(&CERTIFICATION_VISIBILITY),
        ),
    ],
};

/// The complete declared field list of a derived-`Deserialize` struct, read
/// from the list serde hands to `deserialize_struct`. Unlike the keys of one
/// serialized value, it includes fields a `skip_serializing_if` hides, so an
/// optional field added later cannot slip past a drift test just because the
/// probe left it unset.
pub fn declared_struct_fields<T: de::DeserializeOwned>() -> &'static [&'static str] {
    let mut captured = None;
    let _ = T::deserialize(FieldListProbe(&mut captured));
    captured.expect("the type deserializes as a struct")
}

struct FieldListProbe<'a>(&'a mut Option<&'static [&'static str]>);

impl<'de> Deserializer<'de> for FieldListProbe<'_> {
    type Error = de::value::Error;

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
        Err(de::Error::custom("not a struct"))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        *self.0 = Some(fields);
        Err(de::Error::custom("field list captured"))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map enum identifier ignored_any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::certification::{Certification, CertificationGap};
    use crate::composite::{
        score_agent, AgentSubmission, DeclaredMandate, MandateVerdict, Run, ScoreConfig,
    };
    use crate::evidence_coverage::COMPOSITE_SCORE_INVENTORY;
    use crate::roles::RoleContribution;

    fn probe_submission() -> AgentSubmission {
        AgentSubmission {
            agent_id: "probe".to_string(),
            runs: (0..4)
                .map(|seed| Run {
                    returns: (0..60)
                        .map(|i| 0.001 + 0.0004 * ((i + seed) as f64 * 0.7).sin())
                        .collect(),
                    cost: 1.5,
                    ..Run::default()
                })
                .collect(),
            in_sample_trials: 3,
            candidates: Vec::new(),
        }
    }

    /// A row with every optional field set and every nested shape populated, so
    /// the serialized form exercises the whole allowlist.
    fn populated_score() -> CompositeScore {
        let mut score = score_agent(&probe_submission(), &ScoreConfig::default());
        score.bootstrap_error = Some("bootstrap unavailable".to_string());
        score.deflation_error = Some("deflation unavailable".to_string());
        score.selection_error = Some("selection unavailable".to_string());
        score.dsr_ci_low = Some(0.1);
        score.dsr_ci_high = Some(0.9);
        score.dsr_se = Some(0.2);
        score.role_contributions = vec![RoleContribution {
            role: "momentum".to_string(),
            beta_to_team: 0.5,
            mean_return: 0.001,
            periods: 60,
        }];
        score.declared_mandate = Some(DeclaredMandate::RelativeTo {
            benchmark_id: "buy-and-hold".to_string(),
        });
        score.verdict_applied = Some(MandateVerdict::DrawdownCapped {
            max_per_run_drawdown: 0.2,
        });
        score.declared_passed_k = Some(true);
        score.declared_mandate_eligible = Some(false);
        score.declared_mandate_ordinal = Some(1);
        score.certification = Some(Certification {
            mode: "lifecycle-certified/v1".to_string(),
            certified: false,
            lifecycle_warnings: 2,
            withheld: vec![
                CertificationGap::HostIneligible,
                CertificationGap::LifecycleEvidenceAbsent { run: 1 },
                CertificationGap::LifecycleOrderingBlocked {
                    run: 2,
                    block_violations: 3,
                },
            ],
        });
        score
    }

    fn keys<T: Serialize>(value: &T) -> Vec<String> {
        serde_json::to_value(value)
            .expect("serializes")
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    }

    // ---------------------------------------------------------------------
    // Drift guards: a new field fails here until it is declared.
    // ---------------------------------------------------------------------

    #[test]
    fn every_composite_score_field_is_declared() {
        let audit = COMPOSITE_SCORE_VISIBILITY.audit(declared_struct_fields::<CompositeScore>());
        assert!(
            audit.is_complete(),
            "CompositeScore and its visibility allowlist have diverged.\n\
             Fields present but undeclared (withheld from every surface until declared): {:?}\n\
             Fields declared but absent: {:?}",
            audit.undeclared,
            audit.stale
        );
    }

    #[test]
    fn every_nested_record_field_is_declared() {
        for (allowlist, fields) in [
            (
                ROLE_CONTRIBUTION_VISIBILITY,
                declared_struct_fields::<RoleContribution>(),
            ),
            (
                CERTIFICATION_VISIBILITY,
                declared_struct_fields::<Certification>(),
            ),
        ] {
            let audit = allowlist.audit(fields);
            assert!(audit.is_complete(), "{}: {audit:?}", allowlist.document);
        }
        // Internally tagged enums deserialize through `deserialize_any`, so
        // their fields come from one value per variant instead.
        let mut mandate_keys = std::collections::BTreeSet::new();
        for mandate in [
            DeclaredMandate::AbsoluteReturn,
            DeclaredMandate::RelativeTo {
                benchmark_id: "b".to_string(),
            },
            DeclaredMandate::DrawdownCapped {
                max_per_run_drawdown: 0.1,
            },
            DeclaredMandate::OutperformBuyAndHold,
        ] {
            mandate_keys.extend(keys(&mandate));
        }
        for verdict in [
            MandateVerdict::AbsoluteReturn,
            MandateVerdict::RelativeTo {
                benchmark_id: "b".to_string(),
            },
            MandateVerdict::DrawdownCapped {
                max_per_run_drawdown: 0.1,
            },
        ] {
            mandate_keys.extend(keys(&verdict));
        }
        let mandate_keys: Vec<String> = mandate_keys.into_iter().collect();
        assert!(MANDATE_VISIBILITY.audit(&mandate_keys).is_complete());
        let gap_keys: std::collections::BTreeSet<String> = [
            CertificationGap::HostIneligible,
            CertificationGap::LifecycleEvidenceAbsent { run: 0 },
            CertificationGap::LifecycleOrderingBlocked {
                run: 0,
                block_violations: 1,
            },
        ]
        .iter()
        .flat_map(keys)
        .collect();
        let gap_keys: Vec<String> = gap_keys.into_iter().collect();
        assert!(CERTIFICATION_GAP_VISIBILITY.audit(&gap_keys).is_complete());
    }

    #[test]
    fn the_field_list_probe_sees_fields_a_skipped_none_hides() {
        let fields = declared_struct_fields::<CompositeScore>();
        let serialized = keys(&score_agent(&probe_submission(), &ScoreConfig::default()));
        assert!(fields.contains(&"certification"));
        assert!(
            !serialized.contains(&"certification".to_string()),
            "a default row omits certification, which is why the probe reads the declared list"
        );
    }

    #[test]
    fn every_digest_inventory_field_is_also_declared_for_visibility() {
        for (field, _) in COMPOSITE_SCORE_INVENTORY.fields {
            assert!(
                COMPOSITE_SCORE_VISIBILITY.visibility(field).is_some(),
                "{field} is in the evidence inventory but not the visibility allowlist"
            );
        }
    }

    // ---------------------------------------------------------------------
    // The seal itself.
    // ---------------------------------------------------------------------

    #[test]
    fn sealing_a_fully_declared_row_is_byte_identical() {
        let score = populated_score();
        let sealed = seal_score(&score);
        assert!(sealed.report().is_empty(), "{:?}", sealed.report());
        assert_eq!(
            serde_json::to_string(&sealed).unwrap(),
            serde_json::to_string(&score).unwrap()
        );
        assert_eq!(
            serde_json::to_string_pretty(&sealed).unwrap(),
            serde_json::to_string_pretty(&score).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&sealed).unwrap(),
            serde_json::to_value(&score).unwrap()
        );

        let board = vec![
            score.clone(),
            score_agent(&probe_submission(), &ScoreConfig::default()),
        ];
        assert_eq!(
            serde_json::to_string_pretty(&seal_board(&board)).unwrap(),
            serde_json::to_string_pretty(&board).unwrap()
        );
    }

    /// The committed golden boards are the bytes `sharpebench score --json`
    /// prints. Sealing them must reproduce those bytes exactly.
    #[test]
    fn sealing_the_golden_boards_reproduces_their_bytes() {
        for golden in [
            include_str!("../golden/example_submissions.scores.json"),
            include_str!("../golden/synthetic_field.scores.json"),
        ] {
            let board: Vec<CompositeScore> = serde_json::from_str(golden).unwrap();
            let sealed = seal_board(&board);
            assert!(sealed.report().is_empty(), "{:?}", sealed.report());
            assert_eq!(
                serde_json::to_string_pretty(&sealed).unwrap(),
                golden.trim_end()
            );
        }
    }

    /// The row's first required property: adding a field to the underlying
    /// record does not change the projected output.
    #[test]
    fn an_added_field_does_not_change_the_projection() {
        #[derive(serde::Serialize)]
        struct Grown<'a> {
            #[serde(flatten)]
            score: &'a CompositeScore,
            held_out_window_dates: Vec<&'static str>,
        }
        let score = populated_score();
        let grown = Grown {
            score: &score,
            held_out_window_dates: vec!["2027-03-01", "2027-06-30"],
        };
        let sealed = seal(&grown, &COMPOSITE_SCORE_VISIBILITY).unwrap();
        assert_eq!(
            serde_json::to_string(&sealed).unwrap(),
            serde_json::to_string(&seal_score(&score)).unwrap()
        );
        assert_eq!(
            sealed.report().undeclared,
            vec!["held_out_window_dates".to_string()]
        );
    }

    /// The row's second required property: a planted secret in a field the
    /// allowlist does not name never reaches the output, at any depth.
    #[test]
    fn a_planted_secret_in_an_undeclared_field_is_withheld() {
        const SECRET: &str = "canary-7f3e-held-out-scenario";
        let mut value = serde_json::to_value(populated_score()).unwrap();
        value["scenario_instructions"] = serde_json::json!(SECRET);
        value["role_contributions"][0]["hidden_label"] = serde_json::json!(SECRET);
        value["certification"]["withheld"][1]["window_dates"] = serde_json::json!([SECRET]);
        // A visible scalar that grew into a structure is withheld too.
        value["psr"] = serde_json::json!({ "value": 0.5, "note": SECRET });

        let sealed = seal(&value, &COMPOSITE_SCORE_VISIBILITY).unwrap();
        let text = serde_json::to_string(&sealed).unwrap();
        assert!(!text.contains(SECRET), "the secret leaked: {text}");
        assert!(!text.contains("\"psr\""));
        let report = sealed.report();
        assert_eq!(
            report.undeclared,
            vec![
                "certification.withheld.window_dates".to_string(),
                "role_contributions.hidden_label".to_string(),
                "scenario_instructions".to_string(),
            ]
        );
        assert_eq!(report.restructured, vec!["psr".to_string()]);
        assert!(
            text.contains("\"deflated_sharpe\""),
            "declared fields still pass"
        );
    }

    #[test]
    fn a_withheld_field_is_dropped_and_reported() {
        const ALLOW: VisibilityAllowlist = VisibilityAllowlist {
            document: "test",
            fields: &[
                ("shown", Visibility::Visible),
                (
                    "canary",
                    Visibility::Withheld {
                        reason: "the leak tripwire must never reach an entrant",
                    },
                ),
            ],
        };
        let sealed = seal(&serde_json::json!({"shown": 1, "canary": "x"}), &ALLOW).unwrap();
        assert_eq!(serde_json::to_string(&sealed).unwrap(), r#"{"shown":1}"#);
        assert_eq!(sealed.report().withheld, vec!["canary".to_string()]);
    }

    #[test]
    fn numbers_keep_their_exact_bytes() {
        let value = serde_json::json!({
            "a": 1.0, "b": -0.0, "c": 1e300, "d": 5e-324, "e": 18446744073709551615u64,
            "f": -9223372036854775808i64, "g": 0.1, "h": null
        });
        const ALLOW: VisibilityAllowlist = VisibilityAllowlist {
            document: "numbers",
            fields: &[
                ("a", V),
                ("b", V),
                ("c", V),
                ("d", V),
                ("e", V),
                ("f", V),
                ("g", V),
                ("h", V),
            ],
        };
        let sealed = seal(&value, &ALLOW).unwrap();
        assert_eq!(
            serde_json::to_string(&sealed).unwrap(),
            serde_json::to_string(&value).unwrap()
        );
    }
}
