"""Tests for the independent prospective forecast report checker."""

from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def _load_checker():
    spec = importlib.util.spec_from_file_location(
        "prospective_report_checker", HERE / "check-prospective-forecast-report.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


checker = _load_checker()


def _write(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8", newline="\n")


def _ledger(
    agent: str,
    probabilities: tuple[float, float],
    digests: tuple[str, str] = ("a" * 64, "b" * 64),
) -> dict[str, object]:
    contracts = [
        {
            "contract_id": "c1",
            "kind": "probability",
            "scoring_rule": "binary_brier",
            "resolves_at": 10,
        },
        {
            "contract_id": "c2",
            "kind": "probability",
            "scoring_rule": "binary_brier",
            "resolves_at": 20,
        },
    ]
    revisions = [
        {
            "claim_id": claim_id,
            "contract_sha256": digest,
            "prediction": [probability],
            "ordinal": 0,
            "status": "eligible",
        }
        for claim_id, digest, probability in zip(
            ("c1", "c2"), digests, probabilities, strict=True
        )
    ]
    resolutions = [
        {"claim_id": "c1", "status": "resolved", "outcome": 1.0},
        {"claim_id": "c2", "status": "resolved", "outcome": 0.0},
    ]
    return {
        "identity": {"agent_id": agent, "model_id": agent},
        "contracts": contracts,
        "revisions": revisions,
        "resolutions": resolutions,
    }


def _bins(low_probability: float, high_probability: float) -> list[dict[str, object]]:
    return [
        {
            "lower": 0.0,
            "upper": 0.5,
            "n": 1,
            "mean_forecast": low_probability,
            "event_rate": 0.0,
        },
        {
            "lower": 0.5,
            "upper": 1.0,
            "n": 1,
            "mean_forecast": high_probability,
            "event_rate": 1.0,
        },
    ]


def _agent_summary(
    agent: str, brier: float, skill: float, bins: list[dict[str, object]]
) -> dict[str, object]:
    return {
        "agent_id": agent,
        "n_resolved": 2,
        "metrics": [{"scoring_rule": "binary_brier", "n": 2, "mean_loss": brier}],
        "binary_calibration": {
            "n": 2,
            "brier": brier,
            "base_rate": 0.5,
            "reliability": brier,
            "resolution": 0.25,
            "uncertainty": 0.25,
            "brier_skill": skill,
            "bins": bins,
        },
    }


V1_SUPPORT = {
    "n_contracts": 2,
    "contract_sha256": ["a" * 64, "b" * 64],
    "excluded_resolved_by_agent": {"agent-a": 0, "agent-b": 0},
}

V2_SUPPORT = {
    "rule": checker.SUPPORT_RULE_V2,
    "n_contracts": 2,
    "contract_sha256": ["a" * 64, "b" * 64],
    "unresolved_by_agent": {
        agent: {
            "n_unresolved": 0,
            "not_claimed": [],
            "pending": [],
            "cancelled": [],
            "rejected": [],
        }
        for agent in ("agent-a", "agent-b")
    },
    "settlement_status_disagreements": [],
}


def _write_field(
    field: Path, beta_digests: tuple[str, str] = ("a" * 64, "b" * 64)
) -> None:
    _write(
        field / "field-plan.json",
        {
            "models": [{"agent_id": "agent-a"}, {"agent_id": "agent-b"}],
            "contracts": [
                {"contract_id": "c1", "resolves_at": 10},
                {"contract_id": "c2", "resolves_at": 20},
            ],
            "analysis": {
                "bootstrap_seed": 7,
                "bootstrap_samples": 3,
                "confidence": 0.5,
                "familywise_alpha": 0.05,
                "calibration_bins": 2,
                "minimum_comparative_claim_blocks": 30,
            },
        },
    )
    _write(field / "resolved/agent-a.json", _ledger("agent-a", (0.8, 0.2)))
    _write(
        field / "resolved/agent-b.json",
        _ledger("agent-b", (0.6, 0.4), beta_digests),
    )


def _report(schema: str, support: dict[str, object]) -> dict[str, object]:
    return {
        "schema_version": schema,
        "rank_effect": checker.RANK_EFFECT,
        "dependence_unit": checker.DEPENDENCE_UNIT,
        "config": {
            "bootstrap_seed": 7,
            "bootstrap_samples": 3,
            "confidence": 0.5,
            "familywise_alpha": 0.05,
            "calibration_bins": 2,
        },
        "common_support": json.loads(json.dumps(support)),
        "agents": [
            _agent_summary("agent-a", 0.04, 0.84, _bins(0.2, 0.8)),
            _agent_summary("agent-b", 0.16, 0.36, _bins(0.4, 0.6)),
        ],
        "comparisons": [
            {
                "agent_a": "agent-a",
                "agent_b": "agent-b",
                "n_contracts": 2,
                "n_settlement_blocks": 2,
                "mean_loss_difference": -0.12,
                "confidence_lower": -0.12,
                "confidence_upper": -0.12,
                "raw_p_value": 0.25,
                "holm_adjusted_p_value": 0.25,
                "familywise_significant": False,
            }
        ],
    }


class ProspectiveReportCheckerTests(unittest.TestCase):
    def test_committed_prospective_field_recomputes_exactly(self) -> None:
        field = ROOT / "paper/evidence/prospective-forecast-field"
        result = checker.verify(field, field / "report.json")

        self.assertEqual(result["common_support_contracts"], 24)
        self.assertEqual(result["settlement_blocks"], 6)
        self.assertFalse(result["comparative_claim_supported"])

    def test_recomputes_the_report_and_rejects_a_changed_score(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            field = root / "field"
            _write_field(field)
            for schema, support in (
                (checker.REPORT_SCHEMA, V1_SUPPORT),
                (checker.REPORT_SCHEMA_V2, V2_SUPPORT),
            ):
                with self.subTest(schema=schema):
                    report = _report(schema, support)
                    report_path = root / "report.json"
                    _write(report_path, report)

                    result = checker.verify(field, report_path)
                    self.assertEqual(result["status"], "verified")
                    self.assertFalse(result["comparative_claim_supported"])

                    report["agents"][0]["binary_calibration"]["brier"] = 0.05
                    _write(report_path, report)
                    with self.assertRaisesRegex(
                        checker.ReportCheckError, "brier differs"
                    ):
                        checker.verify(field, report_path)

    def test_each_schema_requires_its_own_support_shape(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            field = root / "field"
            _write_field(field)
            report_path = root / "report.json"
            for schema, support in (
                (checker.REPORT_SCHEMA, V2_SUPPORT),
                (checker.REPORT_SCHEMA_V2, V1_SUPPORT),
            ):
                with self.subTest(schema=schema):
                    _write(report_path, _report(schema, support))
                    with self.assertRaises(checker.ReportCheckError):
                        checker.verify(field, report_path)

            _write(
                report_path,
                _report("sharpebench.forecast-quality.v3", V2_SUPPORT),
            )
            with self.assertRaisesRegex(
                checker.ReportCheckError, "unsupported schema"
            ):
                checker.verify(field, report_path)

    def test_v2_support_is_checked_field_by_field(self) -> None:
        charged = json.loads(json.dumps(V2_SUPPORT))
        charged["unresolved_by_agent"]["agent-b"]["n_unresolved"] = 1
        disputed = json.loads(json.dumps(V2_SUPPORT))
        disputed["settlement_status_disagreements"] = [
            {
                "contract_sha256": "b" * 64,
                "resolved_by": ["agent-a"],
                "pending_by": ["agent-b"],
                "cancelled_by": [],
            }
        ]
        reworded = dict(V2_SUPPORT, rule="field-wide intersection")
        leftover = dict(
            V2_SUPPORT, excluded_resolved_by_agent={"agent-a": 0, "agent-b": 0}
        )
        missing = {
            key: value
            for key, value in V2_SUPPORT.items()
            if key != "settlement_status_disagreements"
        }
        with TemporaryDirectory() as directory:
            root = Path(directory)
            field = root / "field"
            _write_field(field)
            report_path = root / "report.json"
            for label, support, message in (
                ("gap charged", charged, "unresolved_by_agent differs"),
                ("disagreement", disputed, "settlement_status_disagreements differs"),
                ("rule", reworded, "common_support.rule differs"),
                ("v1 field", leftover, "fields differ"),
                ("missing field", missing, "settlement_status_disagreements differs"),
            ):
                with self.subTest(label):
                    _write(report_path, _report(checker.REPORT_SCHEMA_V2, support))
                    with self.assertRaisesRegex(checker.ReportCheckError, message):
                        checker.verify(field, report_path)

            report = _report(checker.REPORT_SCHEMA_V2, V2_SUPPORT)
            report["comparisons"][0]["support_gap"] = {
                "agent_a_unresolved": 0,
                "agent_b_unresolved": 0,
            }
            _write(report_path, report)
            with self.assertRaisesRegex(checker.ReportCheckError, "support gap"):
                checker.verify(field, report_path)

    def test_v2_refuses_ledgers_with_different_digests(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            field = root / "field"
            _write_field(field, beta_digests=("a" * 64, "c" * 64))
            report_path = root / "report.json"
            support = dict(
                V2_SUPPORT,
                n_contracts=3,
                contract_sha256=["a" * 64, "b" * 64, "c" * 64],
            )
            _write(report_path, _report(checker.REPORT_SCHEMA_V2, support))
            with self.assertRaisesRegex(
                checker.ReportCheckError, "different contract digests"
            ):
                checker.verify(field, report_path)

    def test_splitmix_reference_stream_is_stable(self) -> None:
        generator = checker._SplitMix64(0)
        self.assertEqual(generator.next(), 0xE220A8397B1DCDAF)
        self.assertEqual(generator.next(), 0x6E789E6AA1B965F4)


if __name__ == "__main__":
    unittest.main()
