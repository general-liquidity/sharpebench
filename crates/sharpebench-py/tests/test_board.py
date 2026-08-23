"""Tests for the board-ranking binding (sharpebench-core's composite surface).

The anchor is the committed golden fixture: ranking the public three-agent
example field from Python must produce exactly the scores the Rust kernel and
the CLI produce. Equality is asserted on *parsed* JSON, not bytes: both sides
serialize floats with serde_json's shortest-round-trip form, so parsing loses
nothing, while byte equality would additionally pin whitespace (the golden is
pretty-printed CLI stdout, the binding returns compact JSON) and would break on
any serializer formatting change that is not a numeric change.
"""

import json
from pathlib import Path

import pytest

sharpebench = pytest.importorskip("sharpebench")

from sharpebench import (  # noqa: E402
    default_score_config,
    never_catastrophic_config,
    rank_board,
    rank_returns,
    score_one,
)

REPO_ROOT = Path(__file__).resolve().parents[3]
SUITE = REPO_ROOT / "suites" / "example_submissions.json"
GOLDEN = REPO_ROOT / "crates" / "sharpebench-core" / "golden" / "example_submissions.scores.json"


def example_field() -> str:
    return SUITE.read_text()


# ------------------------------------------------------------------ golden parity


def test_rank_board_matches_the_committed_golden_scores():
    """The whole point of JSON-in/JSON-out: the Python board IS the CLI board."""
    board = json.loads(rank_board(example_field()))
    golden = json.loads(GOLDEN.read_text())
    assert board == golden


def test_rank_board_orders_the_example_field():
    board = json.loads(rank_board(example_field()))
    assert [s["agent_id"] for s in board] == ["skilled-momentum", "lucky-yolo", "ungated-bot"]
    assert board[0]["rank_eligible"] is True
    assert board[0]["rank_ordinal"] == 1
    assert not any(s["rank_eligible"] for s in board[1:])
    # The headline property survives the binding: the eligible agent does not
    # need the highest raw return to rank first.
    assert board[0]["composite"] == board[0]["deflated_sharpe"]


def test_rank_board_is_deterministic():
    assert rank_board(example_field()) == rank_board(example_field())


# --------------------------------------------------------------------- score_one


def test_score_one_scores_without_field_context():
    subs = json.loads(example_field())
    score = json.loads(score_one(json.dumps(subs[0])))
    assert score["agent_id"] == subs[0]["agent_id"]
    # Field-relative columns keep their fieldless defaults.
    assert score["field_reality_check_p"] == 1.0
    assert score["field_spa_p"] == 1.0
    assert score["field_crowdedness"] is None
    assert score["rank_ordinal"] == 0
    assert score["trials_sr_std_source"] == "configured"


# ----------------------------------------------------------------------- configs


def test_default_config_round_trips():
    cfg = json.loads(default_score_config())
    # Passing the serialized default back must change nothing.
    assert rank_board(example_field(), json.dumps(cfg)) == rank_board(example_field())
    # And it re-serializes to the same config.
    assert json.loads(default_score_config()) == cfg


def test_never_catastrophic_differs_only_in_pass_mode_and_run_drawdown():
    default = json.loads(default_score_config())
    preset = json.loads(never_catastrophic_config(0.10))
    assert preset["pass_mode"] == "any"
    assert preset["mandate"]["max_run_drawdown"] == 0.10
    # Every other field, including the pooled drawdown cap, is untouched.
    preset["pass_mode"] = default["pass_mode"]
    preset["mandate"]["max_run_drawdown"] = default["mandate"]["max_run_drawdown"]
    assert preset == default


def test_config_fields_are_discoverable():
    cfg = json.loads(default_score_config())
    for field in (
        "n_trials",
        "trials_sr_std",
        "dsr_bar",
        "pass_mode",
        "periods_per_year",
        "mandate",
        "shared_run_set",
        "min_field_for_measured_sr_std",
    ):
        assert field in cfg, field
    assert set(cfg["mandate"]) == {"max_drawdown", "max_run_drawdown"}


def test_config_edits_take_effect():
    """An edited config is honored: an impossible DSR bar empties the eligible set."""
    cfg = json.loads(default_score_config())
    cfg["dsr_bar"] = 1.1  # a probability can never clear this
    board = json.loads(rank_board(example_field(), json.dumps(cfg)))
    assert not any(s["rank_eligible"] for s in board)


# ------------------------------------------------------------------ rank_returns


def test_rank_returns_matches_rank_board_on_equivalent_submissions():
    def track(mean, amp, n=60):
        import math

        return [mean + amp * math.sin(i * 0.7) for i in range(n)]

    returns = {
        "steady": [track(0.002, 0.0005) for _ in range(5)],
        "flat": [track(0.0, 0.003) for _ in range(5)],
    }
    subs = [{"agent_id": aid, "runs": [{"returns": r} for r in rs]} for aid, rs in returns.items()]
    assert json.loads(rank_returns(returns)) == json.loads(rank_board(json.dumps(subs)))

    board = json.loads(rank_returns(returns))
    assert {s["agent_id"] for s in board} == {"steady", "flat"}
    top = board[0]
    assert top["agent_id"] == "steady"
    # Empty traces mean the process and trace-derived columns pass/default
    # trivially. That is a property of this input shape, not of the scorer.
    assert top["process_ok"] is True
    assert top["turnover"] == 0.0
    assert top["calibration_brier"] is None


def test_rank_returns_rejects_malformed_values():
    with pytest.raises(ValueError):
        rank_returns({"a": "not a list of runs"})
    with pytest.raises(ValueError):
        rank_returns({1: [[0.01]]})


# ---------------------------------------------------------------- error handling


def test_invalid_json_raises_value_error_not_a_panic():
    with pytest.raises(ValueError):
        rank_board("not json")
    with pytest.raises(ValueError):
        rank_board("{}")  # an object, not an array of submissions
    with pytest.raises(ValueError):
        score_one("[")
    with pytest.raises(ValueError):
        rank_board(example_field(), "{{bad config")
    with pytest.raises(ValueError):
        rank_board(example_field(), '{"n_trials": "fifty"}')
