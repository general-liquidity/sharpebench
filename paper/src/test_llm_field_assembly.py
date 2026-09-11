"""Completeness regressions for the LLM-field assembler.

The script under test is `paper/evidence/assemble_llm_field.py`, which writes
the `llm-field.jsonl` the paper publishes. It is a script rather than an
importable module, so every case here runs it the way an operator does: a copy
in a temporary directory with a fabricated `final/` beside it and the shared
rate card it imports.

Four findings are pinned, all of them the same shape: a published property that
quietly did not hold on some input, so the assembler produced a plausible field
instead of refusing.

  * The field is the Cartesian product of its models and its datasets, and two
    separate memberships were checked instead: the set of models against the
    required models, the set of datasets against the required datasets. A field
    missing a specific cell passed both, as long as every model appeared against
    some dataset and every dataset appeared against some model. The product is
    now required cell by cell and the refusal names the missing ones.
  * Nothing required a (dataset, agent_id) pair to appear once. A repeated pair
    is two score rows for one submission, and every per-cell reader would count
    it twice. No published field has carried one; this is a missing guard, not a
    number that moved.
  * A statistics file naming a model the per-model accounting table does not
    carry was dropped by `continue`, so that run's calls, tokens and spend left
    no trace and the field published totals as if the model had never run. That
    is the free-by-omission form of the zero the pricing refusal already
    refuses, and it now refuses through the same rule.
  * The statistics files had no stated denominator: nothing said how many were
    read into the counters they feed, so a reader could not tell whether any had
    been dropped. The meta record now states the count.

The corrupt-statistics half of that third finding did not reproduce: a stats
file that does not parse has always raised `SystemExit` naming the file, one
line above the `continue` that dropped the unmatched ones. The case below pins
that behaviour so the two dispositions stay distinguishable.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ASSEMBLER = ROOT / "paper/evidence/assemble_llm_field.py"
SHARED = ROOT / "paper/evidence/llm_pricing.py"

MODELS = ("claude-fable-5", "claude-opus-5", "claude-haiku-4-5-20251001")
DATASETS = ("us-indices-1d", "crypto-majors-1d")


def record(model, dataset):
    return {
        "dataset": dataset,
        "agent_id": f"llm-{model}",
        "model": model,
        "deflated_sharpe": 0.5,
        "passed_k": 1,
        "bootstrap_p": 0.04,
        "rank_eligible": True,
        "worst_run_drawdown": 0.1,
        "raw_mean_return": 0.001,
    }


class AssemblerCase(unittest.TestCase):
    """A complete field fixture, and the ways of making it incomplete.

    Every case starts from a fixture the assembler accepts, so a refusal under
    test is caused by the one thing that case changed. `test_the_control_field
    _assembles` is what makes that argument: without it, a case could pass
    because the fixture was broken in some way nobody named.
    """

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        (self.root / "assemble_llm_field.py").write_text(
            ASSEMBLER.read_text(encoding="utf-8"), encoding="utf-8"
        )
        (self.root / "llm_pricing.py").write_text(
            SHARED.read_text(encoding="utf-8"), encoding="utf-8"
        )
        self.final = self.root / "final"
        self.final.mkdir()
        self.write_records(
            [record(m, d) for m in MODELS for d in DATASETS]
        )
        for model in MODELS:
            self.write_cache(model)

    def write_records(self, records):
        (self.final / "llm-field-records-all.jsonl").write_text(
            "".join(json.dumps(r) + "\n" for r in records), encoding="utf-8"
        )

    def write_cache(self, model, tokens_in=1_000_000, tokens_out=100_000):
        (self.final / f"llm-cache-{model}.jsonl").write_text(
            json.dumps({"tokens_in": tokens_in, "tokens_out": tokens_out}) + "\n",
            encoding="utf-8",
        )

    def write_stats(self, name, payload):
        stats = self.final / "llm-stats"
        stats.mkdir(exist_ok=True)
        (stats / name).write_text(
            payload if isinstance(payload, str) else json.dumps(payload),
            encoding="utf-8",
        )

    def run_assembler(self):
        done = subprocess.run(
            [sys.executable, str(self.root / "assemble_llm_field.py")],
            capture_output=True,
            text=True,
        )
        return done.returncode, done.stdout + done.stderr

    def assemble(self):
        code, output = self.run_assembler()
        self.assertEqual(code, 0, output)
        return json.loads(
            (self.final / "llm-field.jsonl").read_text(encoding="utf-8").splitlines()[0]
        )

    def refusal(self):
        code, output = self.run_assembler()
        self.assertEqual(code, 1, output)
        self.assertFalse(
            (self.final / "llm-field.jsonl").exists(),
            "a refused assembly must not leave a field behind",
        )
        return output


class CellCompletenessTests(AssemblerCase):
    def test_the_control_field_assembles(self):
        meta = self.assemble()
        self.assertEqual(meta["datasets"], sorted(DATASETS))

    def test_a_missing_model_dataset_cell_is_refused_and_named(self):
        """The exact defect, in the one input that separates the product from
        the two memberships.

        One cell is dropped, and only one. Every required model still appears in
        the file, against its other dataset, and every required dataset still
        appears, under the other two models, so the model-set and dataset-set
        memberships the assembler used to check both pass on this fixture. It
        was accepted, and the field was published carrying five of its six
        cells.
        """
        kept = [
            record(m, d)
            for m in MODELS
            for d in DATASETS
            if (m, d) != ("claude-opus-5", "crypto-majors-1d")
        ]
        self.write_records(kept)
        self.assertEqual(
            {r["model"] for r in kept}, set(MODELS), "the model set must still pass"
        )
        self.assertEqual(
            {r["dataset"] for r in kept},
            set(DATASETS),
            "the dataset set must still pass",
        )
        output = self.refusal()
        self.assertIn("claude-opus-5/crypto-majors-1d", output)
        self.assertNotIn("claude-opus-5/us-indices-1d", output)

    def test_a_cell_no_model_the_field_names_ran_is_refused(self):
        """The other direction: a cell that is present and not required."""
        rows = [record(m, d) for m in MODELS for d in DATASETS]
        rows.append(record("claude-opus-5", "fx-majors-1d"))
        self.write_records(rows)
        output = self.refusal()
        self.assertIn("claude-opus-5/fx-majors-1d", output)


class PairUniquenessTests(AssemblerCase):
    def test_a_repeated_dataset_agent_pair_is_refused_and_named(self):
        """One submission is one score row. A duplicated pair is counted twice
        by every per-cell reader of the published field, and the product check
        above cannot see it: the duplicate leaves the set of observed cells
        exactly as it was."""
        rows = [record(m, d) for m in MODELS for d in DATASETS]
        rows.append(record("claude-fable-5", "us-indices-1d"))
        self.write_records(rows)
        self.assertEqual(
            {(r["model"], r["dataset"]) for r in rows},
            {(m, d) for m in MODELS for d in DATASETS},
            "the duplicate must not change the observed cells",
        )
        output = self.refusal()
        self.assertIn("us-indices-1d/llm-claude-fable-5", output)

    def test_a_non_llm_row_is_held_to_the_same_pairing(self):
        """The reference-field and luck-floor rows the file carries are
        submissions under the same (dataset, agent_id) pairing, so the guard is
        over all records rather than the LLM rows alone."""
        rows = [record(m, d) for m in MODELS for d in DATASETS]
        reference = dict(record("claude-opus-5", "us-indices-1d"))
        reference["agent_id"] = "buy-and-hold"
        rows.extend([reference, dict(reference)])
        self.write_records(rows)
        output = self.refusal()
        self.assertIn("us-indices-1d/buy-and-hold", output)


class StatisticsAccountingTests(AssemblerCase):
    def test_a_stats_file_naming_a_model_with_no_accounting_row_is_refused(self):
        """A model the per-model table does not name costs a refusal, not zero.

        The table is built from the response caches, so a statistics file naming
        a model with no cache beside it describes a run whose calls, tokens and
        spend the field cannot state. `continue` dropped it and published the
        totals as if that run had not happened, which is the same zero the
        pricing refusal exists to prevent, reached by omission instead of by a
        missing rate card. The model named here is one the rate card prices, so
        the refusal cannot be the pricing gate.
        """
        orphan = "claude-fable-5-20260101"
        self.write_stats(
            "stats-1.json", {"model": orphan, "observations": 40, "cache_hits": 3}
        )
        output = self.refusal()
        self.assertIn(orphan, output)
        self.assertIn("no accounting row", output)
        self.assertNotIn("no rate card", output)

    def test_a_matched_stats_file_lands_in_the_model_it_names(self):
        """The control for the case above, and the reason it is about the join
        and not about statistics files in general."""
        self.write_stats(
            "stats-1.json",
            {"model": "claude-fable-5", "observations": 40, "cache_hits": 3},
        )
        meta = self.assemble()
        self.assertEqual(meta["per_model"]["claude-fable-5"]["observations"], 40)
        self.assertEqual(meta["per_model"]["claude-fable-5"]["cache_hits"], 3)

    def test_the_number_of_statistics_files_read_is_published(self):
        """The denominator for the secondary counters. Without it a reader
        cannot tell how many files fed them, and so cannot tell whether any were
        dropped on the way."""
        for i in range(3):
            self.write_stats(
                f"stats-{i}.json", {"model": "claude-opus-5", "observations": 1}
            )
        meta = self.assemble()
        self.assertEqual(meta["stats_files_read"], 3)
        self.assertEqual(meta["per_model"]["claude-opus-5"]["observations"], 3)

    def test_a_corrupt_stats_file_refuses_and_names_the_file(self):
        """This half of the finding did not reproduce: a stats file that does
        not parse has always refused rather than being skipped. Pinned so the
        two dispositions stay distinguishable, and so a later edit cannot turn
        this one into a `continue` the way the unmatched case was."""
        self.write_stats("stats-broken.json", "{not json")
        output = self.refusal()
        self.assertIn("corrupt stats file", output)
        self.assertIn("stats-broken.json", output)

    def test_a_refused_model_identity_makes_the_field_incomplete(self):
        """A call the provider answered under a model the field does not name is
        billed, recorded in the token counters by the shim and absent from
        `cost_usd`. It is the same kind of incompleteness as an API error, and
        is refused with them rather than published as a complete field."""
        self.write_stats(
            "stats-1.json", {"model": "claude-opus-5", "identity_refusals": 1}
        )
        output = self.refusal()
        self.assertIn("refused model identities", output)


class OneRefusalRuleTests(AssemblerCase):
    def test_both_unaccountable_causes_refuse_through_one_rule(self):
        """The two causes are one rule, stated once.

        A model with no rate card and a model with no accounting row are the
        same unavailability from two sides, and zero is the tempting answer to
        each. Restated, a later edit could repair one and leave the other
        publishing a billed model as free, so both refusals are the same
        sentence and this case reads it in both outputs.
        """
        shared = (
            "spend this field cannot state has no cost it can publish, and "
            "reporting zero would publish calls that were billed as free"
        )
        self.write_stats("stats-1.json", {"model": "claude-fable-5-20260101"})
        self.assertIn(shared, self.refusal())

        self.write_stats("stats-1.json", {"model": "claude-fable-5"})
        self.write_cache("claude-opus-5-1")
        self.write_records(
            [record(m, d) for m in MODELS for d in DATASETS]
        )
        unpriced = self.refusal()
        self.assertIn("no rate card for claude-opus-5-1", unpriced)
        self.assertIn(shared, unpriced)

    def test_the_rule_is_stated_once_in_the_source(self):
        """Text, because the case above cannot tell one statement from two
        identical ones. The sentence both refusals print comes from a single
        function; two copies of it is the desynchronization this guards."""
        source = ASSEMBLER.read_text(encoding="utf-8")
        self.assertEqual(
            source.count("reporting zero would publish calls that were billed as free"),
            1,
            "the refusal is stated twice; a later edit can repair one copy only",
        )
        self.assertEqual(source.count("def refuse_unaccountable("), 1)
        self.assertEqual(source.count("refuse_unaccountable("), 3)


if __name__ == "__main__":
    unittest.main()
