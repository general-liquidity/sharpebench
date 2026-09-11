"""Order-validation regressions for the paid LLM shim.

The shim under test is `examples/llm-agent/llm_agent.py`, the producer behind
the LLM field. Nothing here calls a provider: the module is imported with a
temporary cache and statistics directory, and every API object is a stand-in.

One finding is pinned. The weight of an order was read as
`float(o.get("target_weight", 0.0))`, which turned two replies that state no
weight into weights the model never expressed:

  * an order with no `target_weight` key became a zero-weight order. That is not
    an absent instruction, it is a deliberate one: go flat in that symbol. The
    system prompt tells the model to omit a *symbol* to leave its position
    untouched, so an order naming a symbol and omitting its weight is exactly
    the ambiguous reply, and it was published as an allocation decision.
  * a JSON `true` became 1.0, a full allocation, because `bool` is a subclass of
    `int` in Python and `float(True)` is 1.0. A JSON string was converted too,
    so `"0.5"` was a half allocation from a reply that stated no number.

Both are the project's recurring fail-open shape: a plausible number where the
input does not support one. A weight is now required to be present and to be a
JSON number, and the refusal names the offending order rather than reporting an
anonymous malformed reply.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SHIM = ROOT / "examples/llm-agent/llm_agent.py"

MODEL = "claude-haiku-4-5-20251001"
SYMBOLS = {"AAA"}
OBSERVATION = {
    "symbols": [{"symbol": "AAA", "close_history": [100.0, 101.0, 102.0]}],
    "cash": 1000.0,
    "portfolio": [],
}


def load_shim(tmp):
    argv = sys.argv
    environ = dict(os.environ)
    sys.argv = ["llm_agent.py", MODEL]
    os.environ["LLM_CACHE_DIR"] = str(tmp)
    os.environ["LLM_STATS_DIR"] = str(Path(tmp) / "stats")
    os.environ["LLM_STRIDE"] = "1"
    try:
        spec = importlib.util.spec_from_file_location(f"llm_agent_orders_{id(tmp)}", SHIM)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    finally:
        sys.argv = argv
        os.environ.clear()
        os.environ.update(environ)


class Usage:
    def __init__(self):
        self.input_tokens = 10
        self.output_tokens = 5


class Block:
    def __init__(self, text):
        self.type = "text"
        self.text = text


class Response:
    """A reply that is usable in every respect but the one under test."""

    def __init__(self, text):
        self.model = MODEL
        self.content = [Block(text)]
        self.stop_reason = "end_turn"
        self.usage = Usage()


class Client:
    def __init__(self, answers):
        self.answers = list(answers)
        self.requests = []
        self.messages = self
        # `main` checks the retry setting of every client it is given, so a
        # stand-in reporting none would be refused before the decision loop.
        self.max_retries = 0

    def create(self, **kw):
        self.requests.append(kw)
        return self.answers.pop(0)


def drive(shim, client):
    stdin = io.StringIO(json.dumps(OBSERVATION) + "\n")
    stdout = io.StringIO()
    real_stdin = sys.stdin
    sys.stdin = stdin
    try:
        with contextlib.redirect_stdout(stdout):
            shim.main(client=client)
    finally:
        sys.stdin = real_stdin
    return stdout.getvalue()


def reply(order):
    return json.dumps({"orders": [order]})


class OrderCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.shim = load_shim(self.tmp.name)

    def parse(self, order):
        return self.shim.parse_decision(reply(order), SYMBOLS)

    def refusal(self, order):
        with self.assertRaises(self.shim.MalformedDecision) as caught:
            self.parse(order)
        return str(caught.exception)


class StatedWeightTests(OrderCase):
    def test_an_order_with_no_weight_is_refused_and_named(self):
        """The exact defect. The order is well formed in every other respect,
        so nothing else in the parser can refuse it, and the control below
        parses the same order with a weight of zero stated explicitly."""
        message = self.refusal({"symbol": "AAA", "action": "buy"})
        self.assertIn("states no target_weight", message)
        self.assertIn('"symbol": "AAA"', message)

    def test_an_explicit_zero_weight_is_still_a_decision(self):
        """The control, and the distinction the repair is about: going flat is
        a decision the model can express, and it expresses it by saying so."""
        self.assertEqual(
            self.parse({"symbol": "AAA", "action": "sell", "target_weight": 0.0}),
            [{"symbol": "AAA", "action": "sell", "target_weight": 0.0}],
        )

    def test_a_boolean_weight_is_not_a_full_allocation(self):
        """`float(True)` is 1.0 and `isinstance(True, int)` is True, so a JSON
        `true` was the largest allocation the parser admits."""
        message = self.refusal(
            {"symbol": "AAA", "action": "buy", "target_weight": True}
        )
        self.assertIn("not a JSON number", message)
        self.assertIn("True", message)

    def test_a_string_weight_is_not_a_number(self):
        """`float("0.5")` is 0.5: a half allocation from a reply that stated no
        number."""
        message = self.refusal(
            {"symbol": "AAA", "action": "buy", "target_weight": "0.5"}
        )
        self.assertIn("not a JSON number", message)

    def test_an_integer_weight_is_a_number(self):
        """The bool exclusion must not take integers with it: JSON numbers are
        `int` and `float`, and 1 is a weight the model can state."""
        self.assertEqual(
            self.parse({"symbol": "AAA", "action": "buy", "target_weight": 1}),
            [{"symbol": "AAA", "action": "buy", "target_weight": 1.0}],
        )

    def test_the_bounds_are_still_the_callers(self):
        """`weight_of` decides only whether a number was stated. A stated number
        outside [0, 1] is still a reply-level `None`, as before.

        Negative rather than above one: the agent is long-only and unleveraged,
        and a weight above one is refused by the sum-of-weights rule as well, so
        it would not tell the [0, 1] bound from that one.
        """
        self.assertIsNone(
            self.parse({"symbol": "AAA", "action": "sell", "target_weight": -0.5})
        )

    def test_a_reply_that_is_not_a_decision_is_still_none(self):
        """Nothing to name, so nothing is raised: the `None` contract is
        unchanged for failures that are not about a particular order."""
        self.assertIsNone(self.shim.parse_decision("not json at all", SYMBOLS))
        self.assertIsNone(self.shim.parse_decision('{"orders": 3}', SYMBOLS))


class FaultSiteTests(OrderCase):
    def test_the_run_reports_which_order_it_refused(self):
        """End to end, on the path the harness takes. The decision stays an
        invalid wire message so the Rust transport classifies the run as an
        agent-protocol failure, and it now carries which order caused it."""
        out = drive(self.shim, Client([Response(reply({"symbol": "AAA", "action": "buy"}))]))
        decision = json.loads(out.strip())
        self.assertIn("protocol_error", decision)
        self.assertIn("states no target_weight", decision["protocol_error"])
        self.assertIn("AAA", decision["protocol_error"])
        self.assertEqual(self.shim.STATS["malformed"], 1)

    def test_a_weightless_order_is_not_executed_as_a_hold(self):
        """What the defect published: a decision, with an order in it, at a
        weight the model never stated."""
        out = drive(self.shim, Client([Response(reply({"symbol": "AAA", "action": "buy"}))]))
        self.assertNotIn("orders", json.loads(out.strip()))

    def test_the_control_reply_still_executes(self):
        """The stand-in is not too thin to produce a decision: the same shape
        with a weight stated runs through to orders."""
        order = {"symbol": "AAA", "action": "buy", "target_weight": 0.25}
        out = drive(self.shim, Client([Response(reply(order))]))
        self.assertEqual(json.loads(out.strip())["orders"], [order])
        self.assertEqual(self.shim.STATS["malformed"], 0)


if __name__ == "__main__":
    unittest.main()
