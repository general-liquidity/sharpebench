"""One rate card behind both files that price an LLM call.

`examples/llm-agent/llm_agent.py` meters a running policy and refuses a model
it cannot price. `paper/evidence/assemble_llm_field.py` writes the `cost_usd`
the paper publishes. Both used to carry their own copy of the pricing table and
their own restatement of the alias-expansion rule, and that duplication was
recorded as a limit rather than closed: a future edit to one side could price a
model the other refused, or price the same model differently, and nothing would
have said so. The table, the rule and the acceptance decision now live in
`paper/evidence/llm_pricing.py`, which both import.

This file is the check that keeps it that way. It fails when either consumer
grows a second table, a second copy of the rule, or a second acceptance walk,
and it says which file did it and how the copy differs, rather than leaving the
disagreement to surface later as a wrong number. Two of its cases go further
than the text and drive the assembler as a subprocess against a mutated shared
module, so what is established is that the published cost is computed from that
table at run time and not merely that the file mentions it.

No provider SDK is involved. The shared module imports nothing, the assembler
imports `json`, `sys` and `pathlib`, and this file never imports the shim, so
it runs in the `paper-provenance` job, which installs nothing. The `llm-shim`
job pins `anthropic==0.112.0` for the ceiling and identity regressions that
need a real SDK client; a check that two files agree about a price list has no
use for it.
"""

from __future__ import annotations

import ast
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SHARED = ROOT / "paper/evidence/llm_pricing.py"
SHIM = ROOT / "examples/llm-agent/llm_agent.py"
ASSEMBLER = ROOT / "paper/evidence/assemble_llm_field.py"

# The names that must have exactly one definition, in the shared module. A table
# is a drift risk because its values can differ; the rule and the acceptance
# walk are drift risks because a model one side admits the other can refuse.
SHARED_NAMES = ("PRICING", "SNAPSHOT_SEPARATOR", "SNAPSHOT_DIGITS")
SHARED_FUNCTIONS = ("is_dated_snapshot_of", "lookup_price")

CONSUMERS = {"examples/llm-agent/llm_agent.py": SHIM,
             "paper/evidence/assemble_llm_field.py": ASSEMBLER}


def load_shared():
    spec = importlib.util.spec_from_file_location("llm_pricing_under_test", SHARED)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def assignments(tree, name):
    """Every module-level assignment to `name`, as (lineno, value node)."""
    found = []
    for node in tree.body:
        if not isinstance(node, ast.Assign):
            continue
        for target in node.targets:
            if isinstance(target, ast.Name) and target.id == name:
                found.append((node.lineno, node.value))
    return found


def definitions(tree, name):
    return [
        node.lineno
        for node in tree.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == name
    ]


def imported_from_shared(tree):
    """The names each file takes from the shared module."""
    names = []
    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom) and node.module == "llm_pricing":
            names.extend(alias.name for alias in node.names)
    return names


def describe_table_drift(copy, shared):
    """How a second table differs from the shared one, in the failure message.

    An identical copy is still a defect, because it is free to stop being
    identical, but a copy that already differs is a live mispricing and the
    message says so at the entry.
    """
    if copy == shared:
        return "it duplicates the shared table exactly, and a copy is free to drift"
    lines = []
    for model in sorted(set(copy) | set(shared)):
        if model not in shared:
            lines.append(f"{model}: priced {copy[model]} here, absent from the shared table")
        elif model not in copy:
            lines.append(f"{model}: absent here, priced {shared[model]} in the shared table")
        elif copy[model] != shared[model]:
            lines.append(f"{model}: {copy[model]} here, {shared[model]} in the shared table")
    return "it already disagrees with the shared table at " + "; ".join(lines)


class OneRateCardTests(unittest.TestCase):
    """The table and the rule have one definition, and both files use it."""

    def setUp(self):
        self.shared = load_shared()
        self.trees = {
            label: ast.parse(path.read_text(encoding="utf-8"))
            for label, path in CONSUMERS.items()
        }

    def test_neither_consumer_carries_its_own_rate_card(self):
        for label, tree in self.trees.items():
            for lineno, value in assignments(tree, "PRICING"):
                try:
                    copy = ast.literal_eval(value)
                except ValueError:
                    copy = None
                drift = (
                    describe_table_drift(copy, self.shared.PRICING)
                    if isinstance(copy, dict)
                    else "its contents are not a literal, so they cannot be compared"
                )
                self.fail(
                    f"pricing drift: {label}:{lineno} defines its own PRICING table; "
                    f"{drift}. The rate card is paper/evidence/llm_pricing.py, shared "
                    "with the other file that prices a call, so a model cannot be "
                    "priced by one side and refused or priced differently by the other"
                )

    def test_neither_consumer_restates_the_snapshot_rule(self):
        for label, tree in self.trees.items():
            for name in SHARED_NAMES[1:]:
                for lineno, _ in assignments(tree, name):
                    self.fail(
                        f"pricing drift: {label}:{lineno} defines its own {name}; the "
                        "alias-expansion rule is paper/evidence/llm_pricing.py's, and a "
                        "second copy can admit a served id the other side refuses"
                    )
            for name in SHARED_FUNCTIONS:
                for lineno in definitions(tree, name):
                    self.fail(
                        f"pricing drift: {label}:{lineno} defines its own {name}; the "
                        "acceptance rule is paper/evidence/llm_pricing.py's, and a "
                        "second copy can admit a model the other side refuses"
                    )

    def test_both_consumers_import_the_shared_module(self):
        """The other half of the property: one definition is only one rule if
        both files reach for it."""
        for label, tree in self.trees.items():
            taken = imported_from_shared(tree)
            self.assertTrue(
                taken,
                f"pricing drift: {label} imports nothing from llm_pricing, so whatever "
                "it prices by is not the shared rate card",
            )
            self.assertIn(
                "lookup_price",
                taken,
                f"pricing drift: {label} does not take lookup_price from llm_pricing, so "
                "its acceptance decision is its own",
            )
            for name in taken:
                self.assertTrue(
                    hasattr(self.shared, name),
                    f"{label} imports {name} from llm_pricing, which does not define it",
                )

    def test_the_shared_module_imports_nothing(self):
        """Why sharing was possible at all. The rule was restated in the
        assembler to keep the Anthropic SDK out of a file reader; a module with
        no imports cannot carry anything into either side."""
        tree = ast.parse(SHARED.read_text(encoding="utf-8"))
        imports = [
            node.lineno
            for node in ast.walk(tree)
            if isinstance(node, (ast.Import, ast.ImportFrom))
        ]
        self.assertEqual(
            imports,
            [],
            "paper/evidence/llm_pricing.py imports something; it is shared with an "
            "assembler that reads only files and must stay free of dependencies",
        )

    def test_the_field_the_producer_runs_is_priced(self):
        for model in ("claude-fable-5", "claude-opus-5", "claude-haiku-4-5-20251001"):
            with self.subTest(model=model):
                self.assertIsNotNone(self.shared.lookup_price(model))


class AssemblerReadsTheSharedTableTests(unittest.TestCase):
    """The published number is computed from the shared table at run time.

    The cases above are about text. These drive the assembler as an operator
    does, against a copy of the shared module that has been changed, and observe
    the change in what it refuses and in what it publishes. Without them a
    consumer could import the module and still price by something else.
    """

    MODELS = ("claude-fable-5", "claude-opus-5", "claude-haiku-4-5-20251001")
    DATASETS = ("us-indices-1d", "crypto-majors-1d")
    TOKENS_IN = 1_000_000
    TOKENS_OUT = 100_000

    def layout(self, pricing_source=None):
        """An assembler, a shared module and a complete field fixture in a
        temporary directory."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        (root / "assemble_llm_field.py").write_text(
            ASSEMBLER.read_text(encoding="utf-8"), encoding="utf-8"
        )
        (root / "llm_pricing.py").write_text(
            pricing_source
            if pricing_source is not None
            else SHARED.read_text(encoding="utf-8"),
            encoding="utf-8",
        )
        final = root / "final"
        final.mkdir()
        return root, final

    def fixture(self, final, models):
        records = [
            {
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
            for model in models
            for dataset in self.DATASETS
        ]
        (final / "llm-field-records-all.jsonl").write_text(
            "".join(json.dumps(r) + "\n" for r in records), encoding="utf-8"
        )
        stats = final / "llm-stats"
        stats.mkdir(exist_ok=True)
        for model in models:
            (final / f"llm-cache-{model}.jsonl").write_text(
                json.dumps(
                    {"tokens_in": self.TOKENS_IN, "tokens_out": self.TOKENS_OUT}
                )
                + "\n",
                encoding="utf-8",
            )
            # The assembler reconciles the three kinds of evidence a run leaves
            # and refuses a model missing any of them, so one call has to be
            # recorded in all three or these cases would refuse for a reason
            # that has nothing to do with the rate card.
            (final / f"llm-attempts-{model}.jsonl").write_text(
                json.dumps({"key": "k0", "pid": 1}) + "\n", encoding="utf-8"
            )
            (stats / f"stats-{model}.json").write_text(
                json.dumps({"model": model, "llm_calls": 1}), encoding="utf-8"
            )

    def run_assembler(self, root):
        done = subprocess.run(
            [sys.executable, str(root / "assemble_llm_field.py")],
            capture_output=True,
            text=True,
        )
        return done.returncode, done.stdout + done.stderr

    def published_cost(self, root, final):
        code, output = self.run_assembler(root)
        self.assertEqual(code, 0, output)
        meta = json.loads((final / "llm-field.jsonl").read_text(encoding="utf-8").splitlines()[0])
        return meta["cost_usd_total"], meta["per_model"]

    def test_the_published_cost_is_the_shared_table_applied(self):
        """The rates, not just the model set. Each of the three models bills a
        million input and a hundred thousand output tokens, so the published
        total is a sum this test can compute from the shared table itself."""
        shared = load_shared()
        root, final = self.layout()
        self.fixture(final, self.MODELS)
        total, per_model = self.published_cost(root, final)
        expected = 0.0
        for model in self.MODELS:
            pin, pout = shared.lookup_price(model)
            want = round(self.TOKENS_IN * pin + self.TOKENS_OUT * pout, 4)
            self.assertEqual(per_model[model]["cost_usd"], want)
            expected += want
        self.assertEqual(total, round(expected, 4))

    def test_a_rate_changed_in_the_shared_table_moves_the_published_cost(self):
        """What makes the case above a binding rather than a coincidence: the
        only thing changed is one rate in the shared module, and the published
        number follows it."""
        source = SHARED.read_text(encoding="utf-8").replace(
            '"claude-opus-5": (5.00e-6, 25.00e-6),',
            '"claude-opus-5": (50.00e-6, 25.00e-6),',
        )
        self.assertIn("(50.00e-6, 25.00e-6)", source, "the rate line was not rewritten")
        root, final = self.layout(pricing_source=source)
        self.fixture(final, self.MODELS)
        _, per_model = self.published_cost(root, final)
        self.assertEqual(
            per_model["claude-opus-5"]["cost_usd"],
            round(self.TOKENS_IN * 50.00e-6 + self.TOKENS_OUT * 25.00e-6, 4),
        )

    def test_a_model_dropped_from_the_shared_table_stops_the_assembly(self):
        """Membership, from the same source. The refusal names the model rather
        than one of the four later gates, which is how this case is told apart
        from an incomplete fixture."""
        source = SHARED.read_text(encoding="utf-8").replace(
            '"claude-opus-5": (5.00e-6, 25.00e-6),\n', ""
        )
        self.assertNotIn('"claude-opus-5"', source, "the entry was not removed")
        root, final = self.layout(pricing_source=source)
        self.fixture(final, self.MODELS)
        code, output = self.run_assembler(root)
        self.assertEqual(code, 1)
        self.assertIn("no rate card for claude-opus-5", output)

    def test_the_control_is_the_same_fixture_on_the_unmodified_table(self):
        """The fixture is not refused by something else: unchanged, it
        assembles."""
        root, final = self.layout()
        self.fixture(final, self.MODELS)
        code, output = self.run_assembler(root)
        self.assertEqual(code, 0, output)


if __name__ == "__main__":
    unittest.main()
