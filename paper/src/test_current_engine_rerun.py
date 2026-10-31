"""Regressions for the current-engine rerun path.

The property that matters is the negative one: this path must not be able to
write into frozen evidence. The rest checks the plan and the manifest; no
producer runs here, and nothing committed is written.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

SOURCE = Path(__file__).with_name("make-current-engine-rerun.py")
SPEC = importlib.util.spec_from_file_location("current_engine_rerun", SOURCE)
rerun = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(rerun)


@pytest.mark.parametrize(
    "relative",
    [
        "paper/evidence/final",
        "paper/evidence/final/nested/run",
        "paper/evidence/development/sub",
        "paper/evidence/after-v0.3.0",
        "paper/evidence/scratch/../final",
    ],
)
def test_output_inside_frozen_evidence_is_refused(relative):
    with pytest.raises(rerun.RerunRefusal, match="frozen evidence"):
        rerun.check_output(rerun.REPO / relative)


def test_a_symlink_into_frozen_evidence_is_refused(tmp_path):
    link = tmp_path / "looks-harmless"
    try:
        link.symlink_to(rerun.REPO / "paper/evidence/final", target_is_directory=True)
    except OSError:
        pytest.skip("this host cannot create symlinks")
    with pytest.raises(rerun.RerunRefusal):
        rerun.check_output(link / "run")


def test_a_sibling_directory_is_accepted():
    target = rerun.REPO / "paper/evidence/current-engine"
    assert rerun.check_output(target) == target.resolve()


def test_a_name_that_only_begins_like_a_frozen_directory_is_accepted():
    target = rerun.REPO / "paper/evidence/final-rerun"
    assert rerun.check_output(target) == target.resolve()


def test_an_unknown_dataset_is_refused_before_any_run():
    with pytest.raises(rerun.RerunRefusal, match="unknown datasets"):
        rerun.select_datasets(["us-indicies-1d"])


def test_the_plan_is_every_producer_on_every_selected_dataset(tmp_path):
    jobs = rerun.plan(tmp_path, rerun.select_datasets([]))
    assert len(jobs) == len(rerun.PRODUCERS) * len(rerun.DATASETS)
    assert len({path for _, _, path in jobs}) == len(jobs)


def test_the_manifest_binds_every_output_and_carries_no_clock(tmp_path):
    jobs = rerun.plan(tmp_path, ["us-indices-1w"])
    for _, _, path in jobs:
        path.parent.mkdir(parents=True)
        path.write_text("{}\n", encoding="utf-8")
    first = rerun.manifest("abc", False, jobs)
    assert set(first["outputs"]) == {
        "evidence_sweep/us-indices-1w.jsonl",
        "external_rules_eval/us-indices-1w.jsonl",
    }
    assert first == rerun.manifest("abc", False, jobs)
    jobs[0][2].write_text("{ }\n", encoding="utf-8")
    assert first != rerun.manifest("abc", False, jobs)
