"""Tests for the independent prospective forecast report checker."""

from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

HERE = Path(__file__).resolve().parent


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


def _ledger(agent: str, probabilities: tuple[float, float]) -> dict[str, object]:
    digests = ("a" * 64, "b" * 64)
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


def _agent_summary(agent: str, brier: float, skill: float, bins: list[dict[str, object]]) -> dict[str, object]:
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


class ProspectiveReportCheckerTests(unittest.TestCase):
    def test_recomputes_the_report_and_rejects_a_changed_score(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            field = root / "field"
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
            _write(field / "resolved/agent-b.json", _ledger("agent-b", (0.6, 0.4)))
            report = {
                "schema_version": checker.REPORT_SCHEMA,
                "rank_effect": checker.RANK_EFFECT,
                "dependence_unit": checker.DEPENDENCE_UNIT,
                "config": {
                    "bootstrap_seed": 7,
                    "bootstrap_samples": 3,
                    "confidence": 0.5,
                    "familywise_alpha": 0.05,
                    "calibration_bins": 2,
                },
                "common_support": {
                    "n_contracts": 2,
                    "contract_sha256": ["a" * 64, "b" * 64],
                    "excluded_resolved_by_agent": {"agent-a": 0, "agent-b": 0},
                },
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
            report_path = root / "report.json"
            _write(report_path, report)

            result = checker.verify(field, report_path)
            self.assertEqual(result["status"], "verified")
            self.assertFalse(result["comparative_claim_supported"])

            report["agents"][0]["binary_calibration"]["brier"] = 0.05
            _write(report_path, report)
            with self.assertRaisesRegex(checker.ReportCheckError, "brier differs"):
                checker.verify(field, report_path)

    def test_splitmix_reference_stream_is_stable(self) -> None:
        generator = checker._SplitMix64(0)
        self.assertEqual(generator.next(), 0xE220A8397B1DCDAF)
        self.assertEqual(generator.next(), 0x6E789E6AA1B965F4)


if __name__ == "__main__":
    unittest.main()
