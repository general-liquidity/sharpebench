"""Completeness regressions for the LLM-field assembler.

The script under test is `paper/evidence/assemble_llm_field.py`, which writes
the `llm-field.jsonl` the paper publishes. It is a script rather than an
importable module, so every case here runs it the way an operator does: a copy
in a temporary directory with a fabricated `final/` beside it and the shared
rate card it imports.

Seven findings are pinned, all of them the same shape: a published property that
quietly did not hold on some input, so the assembler produced a plausible field
instead of refusing. The last three survived the ones before them: an
independent re-check found each still live while every case written for the
earlier ones passed, which is why each is reproduced here as a failing case
before it is closed.

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
  * The pricing refusal was applied to a missing model and not to a missing
    usage record. `rec.get("tokens_in", 0)` costed a cache record that states no
    usage at zero, so a call that was billed was published as free -- the same
    fail-open one level below where it had just been closed. Usage evidence is
    now required per record, and an empty cache is decided on its own terms.
  * Uniqueness was enforced on (dataset, agent_id), and completeness on the SET
    of (model, dataset) cells. A cell filed twice under two agent ids passed
    both: the set collapses the duplicate and the pairing is a different
    identity. The cell is now held unique as well, and the pairing is kept
    because it also covers the reference-field and luck-floor rows.

  * Every reconciliation between the score rows and the spend read its roster
    off the files on disk. `per_model` was whatever `llm-cache-*.jsonl` globbed
    and the missing-model refusal fired only from the loop over `stats-*.json`,
    so with neither kind of file present there was no roster and nothing checked
    anything: a complete six-cell score grid published zero accounted models,
    zero calls and $0.00 and exited 0, while the nineteen cases written for the
    six findings above all passed. The roster now comes from the score rows and
    the required cells, and the three kinds of evidence a run leaves -- the
    attempt ledger, the response cache and the per-process statistics -- are
    required to exist and to agree on how many calls each model made.

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
        self.write_cache_records(
            model, [{"tokens_in": tokens_in, "tokens_out": tokens_out}]
        )

    def write_cache_records(self, model, records, *, evidence=True):
        """One model's response cache, and by default the other two evidence
        kinds counting the same calls.

        A run leaves a ledger line before each request, a cache record for each
        answer and a per-process statistics count of each request, and the
        assembler requires the three to agree. So a fixture that writes a cache
        alone is a fixture with two disagreements in it, and every case built on
        it would refuse for a reason it did not choose. `evidence=False` is for
        the cases that mean to leave one kind out.
        """
        (self.final / f"llm-cache-{model}.jsonl").write_text(
            "".join(json.dumps(r) + "\n" for r in records), encoding="utf-8"
        )
        if evidence:
            self.write_attempts(model, len(records))
            self.write_model_stats(model, len(records))

    def write_attempts(self, model, dispatches):
        (self.final / f"llm-attempts-{model}.jsonl").write_text(
            "".join(
                json.dumps({"key": f"k{i}", "pid": 1}) + "\n"
                for i in range(dispatches)
            ),
            encoding="utf-8",
        )

    def write_model_stats(self, model, llm_calls):
        """The statistics file the setUp fixture writes for each model.

        Named after the model so a case can overwrite exactly this one, and
        distinct from the `stats-<n>.json` names the cases add, which carry no
        `llm_calls` and so leave the reconciled totals where they were.
        """
        self.write_stats(
            f"stats-base-{model}.json", {"model": model, "llm_calls": llm_calls}
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


    def test_a_cell_recorded_twice_under_two_agent_ids_is_refused_and_named(self):
        """The cell is the submission; the agent id is not.

        Completeness above is a membership test over the SET of observed cells,
        so a cell carried twice collapses into one member and passes it. The
        (dataset, agent_id) guard is keyed on a different identity, and a second
        row for the same cell under a different agent id satisfies that one too.
        Both gates passed and the field published two score rows for one
        submission, which every per-cell reader counts twice.

        The added row deliberately does not repeat a (dataset, agent_id) pair.
        If it did, this case would assert an outcome two rules produce and would
        pass with the cell guard absent.
        """
        rows = [record(m, d) for m in MODELS for d in DATASETS]
        rerun = record("claude-fable-5", "us-indices-1d")
        rerun["agent_id"] = "llm-claude-fable-5-rerun"
        rows.append(rerun)
        self.write_records(rows)
        pairs = [(r["dataset"], r["agent_id"]) for r in rows]
        self.assertEqual(
            len(pairs),
            len(set(pairs)),
            "the added row must not also repeat a (dataset, agent_id) pair, or "
            "the refusal under test has two possible causes",
        )
        self.assertEqual(
            {(r["model"], r["dataset"]) for r in rows},
            {(m, d) for m in MODELS for d in DATASETS},
            "the duplicate must leave the set of observed cells unchanged",
        )
        output = self.refusal()
        self.assertIn("claude-fable-5/us-indices-1d", output)
        self.assertIn("llm-claude-fable-5-rerun", output)
        self.assertNotIn("(dataset, agent_id)", output)


class UsageEvidenceTests(AssemblerCase):
    """A call whose token counts are unknown has no cost this field can state.

    The rule `price_for` applies to a model no rate card names, one level down.
    `rec.get("tokens_in", 0)` costed a cache record carrying no usage at zero,
    so calls that were billed were published as free.
    """

    def test_a_cache_record_with_no_usage_is_refused_and_named(self):
        """The exact defect: three calls, no usage evidence, $0.00 published."""
        self.write_cache_records(
            "claude-fable-5",
            [{"malformed": False}, {"malformed": False}, {"refusal": False}],
        )
        output = self.refusal()
        self.assertIn("llm-cache-claude-fable-5.jsonl", output)
        self.assertIn("no usage evidence", output)

    def test_the_refusal_names_the_record_inside_the_cache(self):
        """Which call is unmeasured, not merely that one is: a cache is one line
        per call and an operator has to be able to find it."""
        self.write_cache_records(
            "claude-opus-5",
            [
                {"tokens_in": 10, "tokens_out": 5},
                {"tokens_in": 10, "tokens_out": 5},
                {"tokens_in": 10},
            ],
        )
        output = self.refusal()
        self.assertIn("line 3", output)
        self.assertIn("tokens_out", output)

    def test_a_value_that_is_not_a_token_count_is_refused(self):
        """Present is not measured. A null, a string, a negative count or a
        boolean is not a number of tokens, and defaulting it to zero publishes
        the same free call."""
        for value in (None, "1000", -5, True):
            with self.subTest(value=value):
                self.write_cache_records(
                    "claude-haiku-4-5-20251001",
                    [{"tokens_in": value, "tokens_out": 5}],
                )
                self.assertIn("no usage evidence", self.refusal())

    def test_an_empty_cache_is_refused(self):
        """The decision on the record: a cache with no records is refused.

        It is not the same input as a record with no usage, so it is decided
        separately rather than reached by falling through the per-record rule. A
        cache accumulates one line per fresh call across resumes, and every
        model the field publishes carries score rows the harness produced by
        calling it. A cache with zero lines therefore does not measure a model
        that spent nothing: it is the absence of any evidence about a model that
        certainly called, and `cost_usd: 0.0` derived from it is a number nobody
        measured. Refused under the model's own cause so an operator can tell an
        empty file from an unmeasured record.
        """
        self.write_cache_records("claude-opus-5", [])
        output = self.refusal()
        self.assertIn("llm-cache-claude-opus-5.jsonl", output)
        self.assertIn("no calls recorded", output)

    def test_a_measured_cache_is_still_costed(self):
        """The control: usage that is present and well formed still prices, so
        the cases above are about missing evidence and not about caches."""
        self.write_cache_records(
            "claude-fable-5",
            [
                {"tokens_in": 1_000_000, "tokens_out": 0},
                {"tokens_in": 0, "tokens_out": 100_000},
            ],
        )
        meta = self.assemble()
        row = meta["per_model"]["claude-fable-5"]
        self.assertEqual(row["llm_calls"], 2)
        self.assertEqual(row["tokens_in"], 1_000_000)
        self.assertEqual(row["tokens_out"], 100_000)
        self.assertGreater(row["cost_usd"], 0.0)


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

    def test_the_refusal_says_how_much_of_the_run_is_unaccounted_for(self):
        """A stray file and an entire model's spend are different problems, and
        the operator reading the refusal has to be able to tell them apart.

        On the working tree the second is the real case: 262 of 694 statistics
        files name `claude-opus-5`, which has no response cache and therefore no
        accounting row, so an assembled field would have named three models in
        its score rows and two in `per_model`, with a third of the run's calls
        missing from `llm_calls_total` and `cost_usd_total`. The refusal states
        the proportion and what the totals would have omitted, not just that a
        model is unknown.
        """
        orphan = "claude-fable-5-20260101"
        for i in range(3):
            self.write_stats(f"stats-{i}.json", {"model": orphan, "observations": 1})
        self.write_stats("stats-9.json", {"model": "claude-fable-5"})
        output = self.refusal()
        # Three orphan files, one naming a model with an accounting row, and the
        # three the fixture writes to keep each model's evidence kinds agreeing.
        self.assertIn("3 of 7 statistics files", output)
        self.assertIn(f"{orphan} (3)", output)
        self.assertIn("cost_usd_total", output)

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
        self.assertEqual(meta["stats_files_read"], 3 + len(MODELS))
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


class ScoredModelRosterTests(AssemblerCase):
    """A scored model published with no accounting evidence at all.

    Every gate that reconciled score rows against spend read its roster off the
    files on disk. `per_model` was whatever `llm-cache-*.jsonl` globbed, and the
    missing-model refusal fired only from the loop over `stats-*.json`. With
    neither kind of file present there was no roster and nothing to check
    against, so a complete six-cell score grid published `per_model: {}`,
    `llm_calls_total: 0` and `cost_usd_total: 0` and exited 0. The four cases
    below are the reviewer's reproduction table, run against the same fixture:
    only the last of them refused before this change.
    """

    def drop_evidence(self, *models):
        """Remove every evidence kind for `models`, the way an absent run leaves
        the directory: no cache, no ledger, no statistics file."""
        for model in models:
            (self.final / f"llm-cache-{model}.jsonl").unlink()
            (self.final / f"llm-attempts-{model}.jsonl").unlink()
            (self.final / "llm-stats" / f"stats-base-{model}.json").unlink()

    def test_the_complete_control_publishes_the_accounting_it_scores(self):
        """Row 1, and the control the other three are read against.

        Without it, every case below could pass because the fixture refuses for
        some reason nobody named. It also pins the numbers the reviewer's table
        states, so a field that stops accounting for its models is a change in
        these and not only a change in an exit code.
        """
        meta = self.assemble()
        self.assertEqual(len(meta["per_model"]), len(MODELS))
        self.assertEqual(meta["llm_calls_total"], len(MODELS))
        self.assertEqual(meta["cost_usd_total"], 24.0)

    def test_one_model_with_no_evidence_of_any_kind_is_refused(self):
        """Row 2: one cache absent and no statistics file to notice it.

        The score grid is complete and every other gate passes. Before this
        change the field published two accounted models against six scored
        rows, and 2 calls and $9.00 against three models that ran.
        """
        self.drop_evidence("claude-fable-5")
        output = self.refusal()
        self.assertIn("no response cache", output)
        self.assertIn("claude-fable-5", output)

    def test_a_field_with_no_evidence_at_all_is_refused(self):
        """Row 3, the limit of row 2: six scored rows, zero accounted models,
        zero calls and $0.00 published, and 19 assembler tests passing."""
        self.drop_evidence(*MODELS)
        output = self.refusal()
        self.assertIn("no response cache", output)
        for model in MODELS:
            self.assertIn(model, output)

    def test_one_model_with_no_cache_but_a_statistics_file_is_refused(self):
        """Row 4, which refused before this change and still does.

        It is the row that shows the defect was a roster read off the
        directory rather than a missing rule: the same absent cache was caught
        here only because a statistics file happened to name the model. The
        cause is the older one, so this case asserts that refusal and not the
        roster's.
        """
        self.drop_evidence("claude-fable-5")
        self.write_stats("stats-1.json", {"model": "claude-fable-5"})
        output = self.refusal()
        self.assertIn("no accounting row", output)
        self.assertIn("claude-fable-5", output)

    def test_an_accounted_model_the_field_does_not_score_is_refused(self):
        """The reconciliation in the other direction.

        A cache file for a model with no score row builds an accounting row and
        sums its calls and dollars into the totals, so the field reports spend
        against a model no published row explains. The model named is one the
        rate card prices and its three evidence kinds agree, so neither the
        pricing gate nor the reconciliation below can be the cause.
        """
        self.write_cache("claude-haiku-4-5")
        output = self.refusal()
        self.assertIn("no score row", output)
        self.assertIn("claude-haiku-4-5;", output)
        self.assertNotIn("no rate card", output)


class EvidenceReconciliationTests(AssemblerCase):
    """The three evidence kinds a run leaves, held to one count.

    The ledger takes a line under a lock before each request leaves, so it is
    the ceiling and the only record written before the money is spent. The cache
    records the requests that returned an answer. The statistics count each
    request in the process that issued it. A publishable field has no API error,
    no exhausted budget and no refused identity, so all three see the same calls.
    """

    def test_a_model_with_no_attempt_ledger_is_refused(self):
        """A cache and a statistics file that agree still do not say what the
        provider was asked to do: both record what came back."""
        (self.final / "llm-attempts-claude-opus-5.jsonl").unlink()
        output = self.refusal()
        self.assertIn("no attempt ledger", output)
        self.assertIn("llm-attempts-claude-opus-5.jsonl", output)

    def test_a_model_with_an_empty_attempt_ledger_is_refused(self):
        """Empty is its own cause, as an empty cache is: a ledger with no lines
        is the absence of evidence about a model that certainly dispatched, not
        a measurement that it dispatched nothing. Named separately so the
        operator can tell a truncated ledger from one that disagrees."""
        self.write_attempts("claude-opus-5", 0)
        output = self.refusal()
        self.assertIn("no dispatch reserved", output)

    def test_fewer_cache_records_than_reservations_is_refused(self):
        """Spend with no answer recorded. Two requests left, one came back, so
        the published `llm_calls` and `cost_usd` count one of the two calls the
        provider was asked to make and billed for.

        The statistics are moved with the ledger, so the cache is the only kind
        out of step and the refusal has one cause.
        """
        self.write_attempts("claude-opus-5", 2)
        self.write_model_stats("claude-opus-5", 2)
        output = self.refusal()
        self.assertIn("evidence disagrees", output)
        self.assertIn("reserves 2 dispatches", output)
        self.assertIn("cache records 1", output)

    def test_statistics_counting_fewer_dispatches_than_the_ledger_is_refused(self):
        """A process whose statistics file did not survive. The ledger and the
        cache agree on two calls and the statistics account for one, so the
        secondary counters the field publishes describe half the run."""
        self.write_cache_records(
            "claude-opus-5",
            [{"tokens_in": 10, "tokens_out": 5}, {"tokens_in": 10, "tokens_out": 5}],
        )
        self.write_model_stats("claude-opus-5", 1)
        output = self.refusal()
        self.assertIn("evidence disagrees", output)
        self.assertIn("count 1", output)

    def test_statistics_counting_more_dispatches_than_the_ledger_is_refused(self):
        """The other direction: a count of calls nothing reserved. The ledger is
        written under a lock before each request, so a process claiming more
        dispatches than it reserved is counting requests the ceiling never
        admitted."""
        self.write_model_stats("claude-opus-5", 2)
        output = self.refusal()
        self.assertIn("evidence disagrees", output)
        self.assertIn("count 2", output)

    def test_three_agreeing_evidence_kinds_still_assemble(self):
        """The control for this class. Three ledger lines, three cache records
        and statistics counting three, spread over two files to show the
        statistics are summed rather than read from one."""
        self.write_cache_records(
            "claude-opus-5",
            [{"tokens_in": 10, "tokens_out": 5}] * 3,
            evidence=False,
        )
        self.write_attempts("claude-opus-5", 3)
        self.write_model_stats("claude-opus-5", 1)
        self.write_stats("stats-1.json", {"model": "claude-opus-5", "llm_calls": 2})
        meta = self.assemble()
        self.assertEqual(meta["per_model"]["claude-opus-5"]["llm_calls"], 3)


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
        # One definition and nine causes that reach it. The number is pinned so
        # that a cause added later is added to the one rule and not beside it.
        self.assertEqual(source.count("refuse_unaccountable("), 10)


if __name__ == "__main__":
    unittest.main()
