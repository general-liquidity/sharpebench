#!/usr/bin/env python3
"""Independently recompute a prospective-field forecast-quality report."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from collections import defaultdict
from pathlib import Path
from typing import Any

REPORT_SCHEMA = "sharpebench.forecast-quality.v1"
RANK_EFFECT = "reported_only_never_trading_rank"
DEPENDENCE_UNIT = "whole resolution-clock block across assets and questions"


class ReportCheckError(RuntimeError):
    """The report differs from an independent reconstruction of its inputs."""


def _read_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"),
            parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)),
        )
    except (OSError, UnicodeError, json.JSONDecodeError, ValueError) as error:
        raise ReportCheckError(
            f"cannot read strict JSON from {path}: {error}"
        ) from error


def _mapping(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ReportCheckError(f"{label} must be a JSON object")
    return value


def _list(value: object, label: str) -> list[Any]:
    if not isinstance(value, list):
        raise ReportCheckError(f"{label} must be a JSON array")
    return value


def _number(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ReportCheckError(f"{label} must be a number")
    result = float(value)
    if not math.isfinite(result):
        raise ReportCheckError(f"{label} must be finite")
    return result


def _close(
    actual: object, expected: float, label: str, tolerance: float = 1e-12
) -> None:
    value = _number(actual, label)
    if not math.isclose(value, expected, rel_tol=tolerance, abs_tol=tolerance):
        raise ReportCheckError(
            f"{label} differs: report={value!r}, recomputed={expected!r}"
        )


def _mean(values: list[float]) -> float:
    if not values:
        raise ReportCheckError("cannot average an empty collection")
    return sum(values) / len(values)


class _SplitMix64:
    def __init__(self, state: int) -> None:
        self.state = state & ((1 << 64) - 1)

    def next(self) -> int:
        mask = (1 << 64) - 1
        self.state = (self.state + 0x9E3779B97F4A7C15) & mask
        value = self.state
        value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & mask
        value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & mask
        return (value ^ (value >> 31)) & mask

    def below(self, upper: int) -> int:
        return (self.next() * upper) >> 64


def _pair_seed(left: str, right: str) -> int:
    first, second = sorted((left, right))
    digest = hashlib.sha256(f"{first}\0{second}".encode()).digest()
    return int.from_bytes(digest[:8], "big")


def _percentile(sorted_values: list[float], probability: float) -> float:
    index = math.floor((len(sorted_values) - 1) * probability + 0.5)
    return sorted_values[index]


def _rows(
    document: dict[str, Any], expected_contracts: set[str]
) -> dict[str, dict[str, Any]]:
    revisions = _list(document.get("revisions"), "revisions")
    resolutions = _list(document.get("resolutions"), "resolutions")
    contracts = _list(document.get("contracts"), "contracts")
    contract_by_id = {
        str(_mapping(contract, "contract").get("contract_id")): contract
        for contract in contracts
    }
    if set(contract_by_id) != expected_contracts:
        raise ReportCheckError(
            "resolved ledger contract inventory differs from the plan"
        )
    eligible: dict[str, dict[str, Any]] = {}
    for raw in revisions:
        revision = _mapping(raw, "revision")
        if revision.get("status") != "eligible":
            continue
        claim_id = revision.get("claim_id")
        if not isinstance(claim_id, str):
            raise ReportCheckError("eligible forecast revision has no claim ID")
        previous = eligible.get(claim_id)
        if previous is not None:
            prior_ordinal = previous.get("ordinal")
            current_ordinal = revision.get("ordinal")
            if not isinstance(prior_ordinal, int) or not isinstance(
                current_ordinal, int
            ):
                raise ReportCheckError("forecast revision ordinal is not an integer")
            if current_ordinal <= prior_ordinal:
                raise ReportCheckError("forecast revisions are not strictly ordered")
        eligible[claim_id] = revision
    resolution_by_id: dict[str, dict[str, Any]] = {}
    for raw in resolutions:
        resolution = _mapping(raw, "resolution")
        claim_id = resolution.get("claim_id")
        if (
            not isinstance(claim_id, str)
            or claim_id in resolution_by_id
            or resolution.get("status") != "resolved"
        ):
            raise ReportCheckError(
                "resolution records are missing, duplicated, or unresolved"
            )
        resolution_by_id[claim_id] = resolution
    if (
        set(eligible) != expected_contracts
        or set(resolution_by_id) != expected_contracts
    ):
        raise ReportCheckError(
            "resolved ledger does not cover every frozen contract exactly once"
        )

    rows: dict[str, dict[str, Any]] = {}
    for claim_id in expected_contracts:
        revision = eligible[claim_id]
        resolution = resolution_by_id[claim_id]
        contract = _mapping(contract_by_id[claim_id], f"contract {claim_id}")
        prediction = _list(revision.get("prediction"), f"prediction {claim_id}")
        if (
            contract.get("kind") != "probability"
            or contract.get("scoring_rule") != "binary_brier"
        ):
            raise ReportCheckError(f"unsupported prospective contract: {claim_id}")
        if len(prediction) != 1:
            raise ReportCheckError(f"binary prediction has the wrong width: {claim_id}")
        probability = _number(prediction[0], f"probability {claim_id}")
        outcome = _number(resolution.get("outcome"), f"outcome {claim_id}")
        if not 0.0 <= probability <= 1.0 or outcome not in (0.0, 1.0):
            raise ReportCheckError(
                f"probability or outcome is outside its domain: {claim_id}"
            )
        contract_sha256 = revision.get("contract_sha256")
        if not isinstance(contract_sha256, str) or len(contract_sha256) != 64:
            raise ReportCheckError(f"invalid contract digest: {claim_id}")
        rows[contract_sha256] = {
            "claim_id": claim_id,
            "probability": probability,
            "outcome": outcome,
            "loss": (probability - outcome) ** 2,
            "resolves_at": int(
                _number(contract.get("resolves_at"), f"resolves_at {claim_id}")
            ),
        }
    if len(rows) != len(expected_contracts):
        raise ReportCheckError("two frozen contracts share a digest")
    return rows


def _calibration(rows: dict[str, dict[str, Any]], bins: int) -> dict[str, Any]:
    values = [(row["probability"], row["outcome"]) for row in rows.values()]
    n = len(values)
    base_rate = sum(outcome for _, outcome in values) / n
    brier = sum((probability - outcome) ** 2 for probability, outcome in values) / n
    selected: list[list[tuple[float, float]]] = [[] for _ in range(bins)]
    for probability, outcome in values:
        selected[min(math.floor(probability * bins), bins - 1)].append(
            (probability, outcome)
        )
    bin_values = []
    reliability = 0.0
    resolution = 0.0
    for index, members in enumerate(selected):
        if members:
            mean_forecast = sum(value for value, _ in members) / len(members)
            event_rate = sum(value for _, value in members) / len(members)
            weight = len(members) / n
            reliability += weight * (mean_forecast - event_rate) ** 2
            resolution += weight * (event_rate - base_rate) ** 2
        else:
            mean_forecast = None
            event_rate = None
        bin_values.append(
            {
                "lower": index / bins,
                "upper": (index + 1) / bins,
                "n": len(members),
                "mean_forecast": mean_forecast,
                "event_rate": event_rate,
            }
        )
    uncertainty = base_rate * (1.0 - base_rate)
    return {
        "n": n,
        "brier": brier,
        "base_rate": base_rate,
        "reliability": reliability,
        "resolution": resolution,
        "uncertainty": uncertainty,
        "brier_skill": None if uncertainty == 0.0 else 1.0 - brier / uncertainty,
        "bins": bin_values,
    }


def _comparison(
    agent_a: str,
    rows_a: dict[str, dict[str, Any]],
    agent_b: str,
    rows_b: dict[str, dict[str, Any]],
    common: list[str],
    config: dict[str, Any],
) -> dict[str, Any]:
    blocks: dict[int, list[float]] = defaultdict(list)
    for digest in common:
        left = rows_a[digest]
        right = rows_b[digest]
        if left["resolves_at"] != right["resolves_at"]:
            raise ReportCheckError(
                "common contract digest has unequal settlement clocks"
            )
        blocks[left["resolves_at"]].append(left["loss"] - right["loss"])
    ordered_blocks = [blocks[key] for key in sorted(blocks)]
    observed_values = [value for block in ordered_blocks for value in block]
    observed = _mean(observed_values)
    sample_count = int(config["bootstrap_samples"])
    rng = _SplitMix64(int(config["bootstrap_seed"]) ^ _pair_seed(agent_a, agent_b))
    samples: list[float] = []
    null_extreme = 0
    for _ in range(sample_count):
        sample: list[float] = []
        for _ in ordered_blocks:
            sample.extend(ordered_blocks[rng.below(len(ordered_blocks))])
        sample_mean = _mean(sample)
        samples.append(sample_mean)
        centered = sum(value - observed for value in sample) / len(sample)
        if abs(centered) >= abs(observed):
            null_extreme += 1
    samples.sort()
    tail = (1.0 - float(config["confidence"])) / 2.0
    return {
        "agent_a": agent_a,
        "agent_b": agent_b,
        "n_contracts": len(observed_values),
        "n_settlement_blocks": len(ordered_blocks),
        "mean_loss_difference": observed,
        "confidence_lower": _percentile(samples, tail),
        "confidence_upper": _percentile(samples, 1.0 - tail),
        "raw_p_value": (null_extreme + 1.0) / (sample_count + 1.0),
    }


def _holm(comparisons: list[dict[str, Any]], alpha: float) -> None:
    prior = 0.0
    family = len(comparisons)
    order = sorted(range(family), key=lambda index: comparisons[index]["raw_p_value"])
    for rank, index in enumerate(order):
        adjusted = min(
            1.0,
            max(prior, (family - rank) * comparisons[index]["raw_p_value"]),
        )
        comparisons[index]["holm_adjusted_p_value"] = adjusted
        comparisons[index]["familywise_significant"] = adjusted <= alpha
        prior = adjusted


def _compare_calibration(actual: object, expected: dict[str, Any], label: str) -> None:
    record = _mapping(actual, label)
    if record.get("n") != expected["n"]:
        raise ReportCheckError(f"{label}.n differs")
    for key in ("brier", "base_rate", "reliability", "resolution", "uncertainty"):
        _close(record.get(key), expected[key], f"{label}.{key}")
    if expected["brier_skill"] is None:
        if record.get("brier_skill") is not None:
            raise ReportCheckError(f"{label}.brier_skill must be null")
    else:
        _close(
            record.get("brier_skill"), expected["brier_skill"], f"{label}.brier_skill"
        )
    bins = _list(record.get("bins"), f"{label}.bins")
    if len(bins) != len(expected["bins"]):
        raise ReportCheckError(f"{label}.bins has the wrong length")
    for index, (raw, wanted) in enumerate(zip(bins, expected["bins"], strict=True)):
        item = _mapping(raw, f"{label}.bins[{index}]")
        if item.get("n") != wanted["n"]:
            raise ReportCheckError(f"{label}.bins[{index}].n differs")
        for key in ("lower", "upper", "mean_forecast", "event_rate"):
            if wanted[key] is None:
                if item.get(key) is not None:
                    raise ReportCheckError(f"{label}.bins[{index}].{key} must be null")
            else:
                _close(item.get(key), wanted[key], f"{label}.bins[{index}].{key}")


def verify(field_dir: Path, report_path: Path) -> dict[str, Any]:
    plan = _mapping(_read_json(field_dir / "field-plan.json"), "field plan")
    report = _mapping(_read_json(report_path), "forecast-quality report")
    analysis = _mapping(plan.get("analysis"), "field plan analysis")
    if report.get("schema_version") != REPORT_SCHEMA:
        raise ReportCheckError("forecast-quality report has an unsupported schema")
    if (
        report.get("rank_effect") != RANK_EFFECT
        or report.get("dependence_unit") != DEPENDENCE_UNIT
    ):
        raise ReportCheckError("report changes the frozen rank or dependence semantics")
    expected_config = {
        "bootstrap_seed": analysis.get("bootstrap_seed"),
        "bootstrap_samples": analysis.get("bootstrap_samples"),
        "confidence": analysis.get("confidence"),
        "familywise_alpha": analysis.get("familywise_alpha"),
        "calibration_bins": analysis.get("calibration_bins"),
    }
    if report.get("config") != expected_config:
        raise ReportCheckError(
            "report configuration differs from the preregistered plan"
        )
    models = _list(plan.get("models"), "field plan models")
    agent_ids = sorted(
        str(_mapping(model, "model").get("agent_id")) for model in models
    )
    contracts = _list(plan.get("contracts"), "field plan contracts")
    contract_ids = {
        str(_mapping(contract, "contract").get("contract_id")) for contract in contracts
    }
    if len(agent_ids) != len(set(agent_ids)) or len(contract_ids) != len(contracts):
        raise ReportCheckError("field plan repeats an agent or contract")

    rows_by_agent = {
        agent_id: _rows(
            _mapping(_read_json(field_dir / "resolved" / f"{agent_id}.json"), agent_id),
            contract_ids,
        )
        for agent_id in agent_ids
    }
    common = sorted(set.intersection(*(set(rows) for rows in rows_by_agent.values())))
    support = _mapping(report.get("common_support"), "common_support")
    if (
        support.get("n_contracts") != len(common)
        or support.get("contract_sha256") != common
    ):
        raise ReportCheckError(
            "report common support differs from the resolved ledgers"
        )
    excluded = support.get("excluded_resolved_by_agent")
    expected_excluded = {
        agent_id: len(rows_by_agent[agent_id]) - len(common) for agent_id in agent_ids
    }
    if excluded != expected_excluded:
        raise ReportCheckError("report excluded-support counts differ")

    report_agents = {
        str(_mapping(raw, "agent summary").get("agent_id")): _mapping(
            raw, "agent summary"
        )
        for raw in _list(report.get("agents"), "agents")
    }
    if set(report_agents) != set(agent_ids):
        raise ReportCheckError("report agent inventory differs from the field plan")
    recomputed_brier: dict[str, float] = {}
    for agent_id in agent_ids:
        expected = _calibration(
            rows_by_agent[agent_id], int(expected_config["calibration_bins"])
        )
        actual = report_agents[agent_id]
        if actual.get("n_resolved") != len(rows_by_agent[agent_id]):
            raise ReportCheckError(f"{agent_id}.n_resolved differs")
        metrics = [
            _mapping(metric, f"{agent_id}.metric")
            for metric in _list(actual.get("metrics"), f"{agent_id}.metrics")
            if _mapping(metric, f"{agent_id}.metric").get("scoring_rule")
            == "binary_brier"
        ]
        if len(metrics) != 1 or metrics[0].get("n") != expected["n"]:
            raise ReportCheckError(f"{agent_id} has the wrong binary-Brier metric row")
        _close(metrics[0].get("mean_loss"), expected["brier"], f"{agent_id}.mean_loss")
        _compare_calibration(actual.get("binary_calibration"), expected, agent_id)
        recomputed_brier[agent_id] = expected["brier"]

    recomputed = []
    for left in range(len(agent_ids)):
        for right in range(left + 1, len(agent_ids)):
            recomputed.append(
                _comparison(
                    agent_ids[left],
                    rows_by_agent[agent_ids[left]],
                    agent_ids[right],
                    rows_by_agent[agent_ids[right]],
                    common,
                    expected_config,
                )
            )
    _holm(recomputed, float(expected_config["familywise_alpha"]))
    actual_comparisons = {
        (str(item.get("agent_a")), str(item.get("agent_b"))): item
        for item in (
            _mapping(raw, "comparison")
            for raw in _list(report.get("comparisons"), "comparisons")
        )
    }
    if set(actual_comparisons) != {
        (item["agent_a"], item["agent_b"]) for item in recomputed
    }:
        raise ReportCheckError("report pairwise comparison inventory differs")
    for expected in recomputed:
        label = f"{expected['agent_a']} vs {expected['agent_b']}"
        actual = actual_comparisons[(expected["agent_a"], expected["agent_b"])]
        for key in ("n_contracts", "n_settlement_blocks", "familywise_significant"):
            if actual.get(key) != expected[key]:
                raise ReportCheckError(f"{label}.{key} differs")
        for key in (
            "mean_loss_difference",
            "confidence_lower",
            "confidence_upper",
            "raw_p_value",
            "holm_adjusted_p_value",
        ):
            _close(actual.get(key), expected[key], f"{label}.{key}")

    blocks = len(
        {
            int(_number(contract.get("resolves_at"), "resolves_at"))
            for contract in contracts
        }
    )
    minimum_blocks = int(
        _number(analysis.get("minimum_comparative_claim_blocks"), "minimum blocks")
    )
    return {
        "schema_version": "sharpebench.prospective-forecast-report-check.v1",
        "status": "verified",
        "rank_effect": RANK_EFFECT,
        "agents": agent_ids,
        "common_support_contracts": len(common),
        "settlement_blocks": blocks,
        "minimum_comparative_claim_blocks": minimum_blocks,
        "comparative_claim_supported": blocks >= minimum_blocks,
        "brier": recomputed_brier,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--field-dir", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    try:
        result = verify(arguments.field_dir, arguments.report)
    except ReportCheckError as error:
        parser.error(str(error))
    payload = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if arguments.output is not None:
        arguments.output.write_text(payload, encoding="utf-8", newline="\n")
    print(payload, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
