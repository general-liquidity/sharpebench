"""Call-ceiling regressions for the paid LLM shim.

The shim under test is `examples/llm-agent/llm_agent.py`, the producer behind
the LLM field. Nothing here calls a provider: the module is imported with a
temporary cache and statistics directory and driven with a stand-in client
through the `client` parameter of `main`.

Two findings are pinned.

The ceiling was `len(cache) >= MAX_CALLS`, evaluated against the response
cache, while the billable request was sent further down and the cache was
appended only after a usable reply came back. A call that failed therefore left
the cache unchanged, so the harness retrying the subprocess dispatched again
under the same allowance, and a field could spend an unbounded multiple of its
declared ceiling. The ceiling now counts reservations written before each
dispatch, in a ledger that survives the process the way the cache does.

The reservation then bounded dispatches from this process rather than provider
requests, because the client was a bare `anthropic.Anthropic()` and the SDK
retries some failures itself (two by default), billing each one under the one
reservation. The client now sets `max_retries=0`. The load-bearing case is
driven through a real SDK client over a stand-in HTTP transport, so it is the
SDK's own retry behaviour being observed and not a restatement of the setting.

The SDK behaviour asserted here is `anthropic` 0.112.0's, the version the
retry reading was taken from and the version CI pins for this file:
`DEFAULT_MAX_RETRIES` is 2, and `_base_client` loops `range(max_retries + 1)`
over 408, 409, 429, every 5xx, connection faults and timeouts. anthropic 1.x is
a different SDK (it depends on httpx2) and its retry semantics are not assumed
from this reading.

A later review added two more. The reservation advanced a count read once at
process start, so the ceiling held only because the producer spawns shims one
at a time; the count is now re-read from the ledger inside an exclusive lock,
and a second reservation is refused rather than silently sharing a unit. And a
malformed reply's record carried its tokens but not its cost, so replaying that
decision reported a billed call as free.

A later review added one more, about what a call cost rather than how many were
made. `price_for` walked the pricing table by prefix and answered `(0.0, 0.0)`
for a model no entry matched, so a run on an unpriced model reported every call
as free and that zero was published as its spend; the prefix walk also billed a
model whose name extends a priced one at the other model's card. The rate card
is now matched by the model-identity rule, exactly or as a dated snapshot, and
an unpriced model refuses the run before the first observation is read.

The pin protects CI, not a paid run, which imports whatever the operator has.
That is what `assert_no_provider_retries` is for, and three cases here drive the
refusal rather than the happy path: a client that accepts `max_retries` and
ignores it, one that exposes no readable setting at all, and one handed to
`main` by a caller instead of built by the run. The check sits on `main` as
well as in `build_client`, so a caller-supplied client is on the same footing as
one the run constructs; every case here drives the entry point rather than the
helper.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
import unittest.mock
from pathlib import Path

import httpx

ROOT = Path(__file__).resolve().parents[2]
SHIM = ROOT / "examples/llm-agent/llm_agent.py"

MODEL = "claude-haiku-4-5-20251001"
OBSERVATION = {
    "symbols": [{"symbol": "AAA", "close_history": [100.0, 101.0, 102.0]}],
    "cash": 1000.0,
    "portfolio": [],
}


def load_shim(tmp, max_calls="1", model=MODEL):
    """Import the shim as a fresh module bound to a throwaway cache directory.

    Re-importing against the same directory is how a harness retry is modeled:
    a new process, the same model, the same cache and the same ledger.
    """
    argv = sys.argv
    environ = dict(os.environ)
    sys.argv = ["llm_agent.py", model]
    os.environ["LLM_CACHE_DIR"] = str(tmp)
    os.environ["LLM_STATS_DIR"] = str(Path(tmp) / "stats")
    os.environ["LLM_MAX_CALLS"] = max_calls
    os.environ["LLM_STRIDE"] = "1"
    try:
        spec = importlib.util.spec_from_file_location(f"llm_agent_budget_{id(tmp)}", SHIM)
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
    """The parts of an API response the shim reads."""

    def __init__(self, text):
        self.model = MODEL
        self.content = [Block(text)]
        self.stop_reason = "end_turn"
        self.usage = Usage()


class ReplyUnder(Response):
    """A usable reply carrying a chosen served id.

    The pricing cases run under models other than `MODEL`, and the identity rule
    would refuse a reply naming a different one. Answering under the requested
    id takes that cause off the table, so a pricing case that is not refused
    completes and fails on its own assertion.
    """

    def __init__(self, model, text='{"orders":[]}'):
        super().__init__(text)
        self.model = model


class Client:
    """A stand-in provider. Records every request and answers as instructed."""

    def __init__(self, answers):
        self.answers = list(answers)
        self.requests = []
        self.messages = self
        # Every client `main` is given is checked, supplied or built, so a
        # stand-in that reports nothing is refused before the decision loop
        # runs. The cases about the ceiling and the ledger are not about the
        # check, so the default stand-in reports the compliant setting; the
        # three classes below vary it deliberately.
        self.max_retries = 0

    def create(self, **kw):
        self.requests.append(kw)
        answer = self.answers.pop(0)
        if isinstance(answer, Exception):
            raise answer
        return answer


def connection_error(shim):
    """The SDK's own transport failure, built the way the SDK builds it."""
    import httpx

    request = httpx.Request("POST", "https://api.anthropic.com/v1/messages")
    return shim.anthropic.APIConnectionError(request=request)


def message_payload(text):
    """A Messages response body, so the real SDK parses it rather than a mock."""
    return {
        "id": "msg_test",
        "type": "message",
        "role": "assistant",
        "model": MODEL,
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
        "stop_sequence": None,
        "usage": {"input_tokens": 10, "output_tokens": 5},
    }


# The three stand-in SDKs the runtime check is exercised against. All of them
# answer normally, so a run that is not refused completes and records a
# reservation. That is deliberate: removing the check must make these cases fail
# because nothing refused, not because the stand-in was too thin to dispatch.


class Honouring(Client):
    """Takes `max_retries` and reports it back, as `anthropic` 0.112.0 does."""

    def __init__(self, **kw):
        super().__init__([Response('{"orders":[]}')])
        self.max_retries = kw.get("max_retries")


class Ignoring(Client):
    """Accepts `max_retries` and does not honour it.

    The shape a future SDK takes if it renames the knob, or drops it from the
    constructor's effect while still tolerating the keyword. Without the check
    the ceiling is back to billing several requests per reserved unit, and
    nothing says so.
    """

    def __init__(self, **kw):
        super().__init__([Response('{"orders":[]}')])
        self.max_retries = 2


class Opaque(Client):
    """Exposes no readable retry setting at all, and answers anyway."""

    def __init__(self, **kw):
        super().__init__([Response('{"orders":[]}')])
        del self.max_retries


def sdk_client(shim, http_client):
    """The shim's own client, over a stand-in transport and a dummy key.

    Built through `build_client`'s setting rather than around it: the retry
    policy under test is the one the shim ships.
    """
    return shim.anthropic.Anthropic(
        api_key="test-key-not-a-credential",
        max_retries=shim.PROVIDER_MAX_RETRIES,
        http_client=http_client,
    )


def bar(index, tag):
    """One observation. `tag` moves the price, so the prompt, and with it the
    cache key, differs from every bar carrying another tag."""
    obs = json.loads(json.dumps(OBSERVATION))
    if tag is not None:
        obs["symbols"][0]["close_history"].append(103.0 + 10.0 * tag + index)
    return obs


def drive(shim, client, observations=1, tag=None):
    """Run the decision loop over `observations` bars."""
    stdin = io.StringIO("\n".join(json.dumps(bar(i, tag)) for i in range(observations)))
    stdout = io.StringIO()
    real_stdin = sys.stdin
    sys.stdin = stdin
    try:
        with contextlib.redirect_stdout(stdout):
            shim.main(client=client)
    finally:
        sys.stdin = real_stdin
    return stdout.getvalue()


class CallCeilingCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def ledger_lines(self, shim):
        if not shim.ATTEMPTS_PATH.exists():
            return []
        return [
            line
            for line in shim.ATTEMPTS_PATH.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]


class CallCeilingTests(CallCeilingCase):
    def test_a_failed_dispatch_spends_its_unit_and_a_retry_cannot_respend_it(self):
        """The exact defect: the failure cached nothing, so the retry was free.

        One dispatch is allowed. The first process spends it on a call that
        fails. The second process, which is what the harness starts after the
        transport failure, must refuse before touching the provider.
        """
        first = load_shim(self.tmp.name, max_calls="1")
        failing = Client([connection_error(first)])
        with self.assertRaises(RuntimeError):
            drive(first, failing)
        self.assertEqual(len(failing.requests), 1, "the first process did dispatch")
        self.assertEqual(len(self.ledger_lines(first)), 1, "the dispatch is on the ledger")
        self.assertEqual(first.load_cache(), {}, "a failed call caches nothing")

        second = load_shim(self.tmp.name, max_calls="1")
        retry = Client([Response('{"orders":[]}')])
        with self.assertRaises(RuntimeError) as caught:
            drive(second, retry)
        self.assertIn("budget exhausted", str(caught.exception))
        self.assertEqual(
            retry.requests, [], "the allowance was already spent; nothing may be sent"
        )
        self.assertEqual(second.STATS["budget_exhausted"], 1)

    def test_the_ceiling_counts_dispatches_not_cached_results(self):
        """Two dispatches, one cached result, and the allowance is gone.

        A cache size of one under a ceiling of two used to read as one unit
        left. The ledger records both dispatches, so the third process is
        refused even though only one decision was ever cached.
        """
        failed = load_shim(self.tmp.name, max_calls="2")
        with self.assertRaises(RuntimeError):
            drive(failed, Client([connection_error(failed)]))

        answered = load_shim(self.tmp.name, max_calls="2")
        client = Client([Response('{"orders":[]}')])
        drive(answered, client, observations=1, tag=1)
        self.assertEqual(len(client.requests), 1)
        self.assertEqual(answered.STATS["calls_reserved"], 2)
        self.assertEqual(len(self.ledger_lines(answered)), 2)
        self.assertEqual(len(answered.load_cache()), 1, "one dispatch cached nothing")

        third = load_shim(self.tmp.name, max_calls="2")
        refused = Client([Response('{"orders":[]}')])
        with self.assertRaises(RuntimeError) as caught:
            drive(third, refused, observations=1, tag=2)
        self.assertIn("budget exhausted", str(caught.exception))
        self.assertEqual(refused.requests, [])

    def test_a_cached_decision_spends_nothing(self):
        """The ceiling must not defeat the cache: a replay is not a dispatch.

        The same observation twice under stride 1 is one request and one
        reservation, and the second bar is answered from the cache.
        """
        shim = load_shim(self.tmp.name, max_calls="1")
        client = Client([Response('{"orders":[]}')])
        drive(shim, client, observations=2)
        self.assertEqual(len(client.requests), 1)
        self.assertEqual(shim.STATS["cache_hits"], 1)
        self.assertEqual(len(self.ledger_lines(shim)), 1)

    def test_the_ledger_survives_the_process_and_names_its_configuration(self):
        shim = load_shim(self.tmp.name, max_calls="1")
        client = Client([Response('{"orders":[]}')])
        drive(shim, client)
        record = json.loads(self.ledger_lines(shim)[0])
        self.assertEqual(record["model_requested"], shim.REQUESTED_MODEL)
        self.assertEqual(record["scaffold_version"], shim.SCAFFOLD_VERSION)

        reopened = load_shim(self.tmp.name, max_calls="1")
        self.assertEqual(reopened.load_attempt_count(), 1)

    def test_the_ledger_is_separate_from_the_response_cache(self):
        """Deleting one must not silently restore the other's allowance."""
        shim = load_shim(self.tmp.name, max_calls="1")
        self.assertNotEqual(shim.ATTEMPTS_PATH, shim.CACHE_PATH)
        client = Client([Response('{"orders":[]}')])
        drive(shim, client)
        self.assertTrue(shim.ATTEMPTS_PATH.exists() and shim.CACHE_PATH.exists())


class LedgerOwnershipTests(CallCeilingCase):
    """One unit cannot be spent twice, whoever else shares the ledger."""

    def test_a_unit_another_shim_spent_is_seen_at_reservation_time(self):
        """The defect: the count was read once, at process start.

        This process starts with an empty ledger under a ceiling of one, so its
        startup reading says a unit is free. Another shim then takes it. The
        reservation must read the ledger again and refuse, rather than dispatch
        on a number that was true when the process began.
        """
        shim = load_shim(self.tmp.name, max_calls="1")
        self.assertEqual(shim.load_attempt_count(), 0, "the run began with a free unit")
        shim.ATTEMPTS_PATH.parent.mkdir(parents=True, exist_ok=True)
        with shim.ATTEMPTS_PATH.open("a", encoding="utf-8") as f:
            f.write(json.dumps({"key": "taken-by-another-shim"}) + chr(10))

        client = Client([Response('{"orders":[]}')])
        with self.assertRaises(RuntimeError) as caught:
            drive(shim, client)
        self.assertIn("budget exhausted", str(caught.exception))
        self.assertEqual(client.requests, [], "the unit was already spent")
        self.assertEqual(shim.STATS["budget_exhausted"], 1)
        self.assertEqual(len(self.ledger_lines(shim)), 1, "nothing was appended")

    def test_a_reservation_is_refused_while_another_holds_the_ledger(self):
        """Two shims cannot be between the count and the append at once.

        The allowance is eight and one unit is spent, so nothing here is short
        of budget and the reservation attempted under the held lock is one that
        otherwise succeeds, as the second half of the case shows. Exclusive
        ownership is therefore the only thing that can refuse it.

        Deterministic on purpose. A threaded version of this assertion, eight
        reservations released together against a ceiling of one, was tried
        first and discarded: with the ceiling still in place, "exactly one
        unit" is produced by the lock, by the ceiling, or by threads that
        happened not to interleave, and it caught a build with `O_EXCL`
        removed in four runs out of six.
        """
        shim = load_shim(self.tmp.name, max_calls="8")
        with shim.ledger_lock():
            with self.assertRaises(shim.LedgerBusy) as caught:
                shim.reserve_call("while-held")
            self.assertIn(str(os.getpid()), str(caught.exception))
            self.assertEqual(self.ledger_lines(shim), [], "nothing was appended")
        self.assertEqual(
            shim.reserve_call("while-held"), 1, "the same reservation then works"
        )
        self.assertEqual(len(self.ledger_lines(shim)), 1)

    def test_a_lock_left_behind_refuses_rather_than_being_broken(self):
        """A lock nobody can prove is dead is refused, not removed.

        Breaking it puts two writers back on one allowance, which is what it
        exists to prevent, so the refusal names the holder and the file an
        operator has to clear.
        """
        shim = load_shim(self.tmp.name, max_calls="1")
        shim.LEDGER_LOCK_PATH.parent.mkdir(parents=True, exist_ok=True)
        shim.LEDGER_LOCK_PATH.write_text(
            json.dumps({"pid": 4242, "started_ns": 1}), encoding="utf-8"
        )
        with self.assertRaises(shim.LedgerBusy) as caught:
            shim.reserve_call("key")
        message = str(caught.exception)
        self.assertIn("pid 4242", message)
        self.assertIn(str(shim.LEDGER_LOCK_PATH), message)
        self.assertEqual(self.ledger_lines(shim), [], "nothing was reserved")
        self.assertTrue(shim.LEDGER_LOCK_PATH.exists(), "the lock was not broken")

    def test_the_release_outlasts_a_refused_shim_reading_the_holder(self):
        """Releasing must not be defeated by the refusal it races with.

        A refused shim reads the holder document, and Windows refuses to delete
        a file another handle holds (WinError 32). The holder retries rather
        than stranding a lock that would refuse every later reservation, which
        eight contending threads reproduced before the retry existed. The
        reader here is closed on a timer while the release is in flight, so the
        case is deterministic rather than timing-dependent.

        On a platform that allows an unlinked file to stay open, the first
        attempt succeeds and this asserts the same end state for free.
        """
        shim = load_shim(self.tmp.name, max_calls="8")
        with shim.ledger_lock():
            reader = open(shim.LEDGER_LOCK_PATH, encoding="utf-8")
            self.addCleanup(reader.close)
            threading.Timer(0.15, reader.close).start()
        self.assertFalse(
            shim.LEDGER_LOCK_PATH.exists(), "the lock was released, not stranded"
        )
        self.assertEqual(shim.reserve_call("after-release"), 1)

    def test_the_lock_is_released_when_the_reservation_returns(self):
        """Strictness must not defeat the ledger: a reservation that completed
        leaves nothing behind for the next one to trip over."""
        shim = load_shim(self.tmp.name, max_calls="2")
        shim.reserve_call("first")
        self.assertFalse(shim.LEDGER_LOCK_PATH.exists())
        self.assertEqual(shim.reserve_call("second"), 2)


class MalformedCostTests(CallCeilingCase):
    def test_a_malformed_reply_records_what_the_call_cost(self):
        """A billed call whose reply did not parse is not a free call.

        The decision has to stay unparseable, so the cost rides on the cache
        record, which is what the accounting reads. It used to carry tokens but
        no cost, so replaying the decision reported none.
        """
        shim = load_shim(self.tmp.name, max_calls="1")
        drive(shim, Client([Response("not json at all")]))
        self.assertEqual(shim.STATS["malformed"], 1)
        records = [
            json.loads(line)
            for line in shim.CACHE_PATH.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        self.assertEqual(len(records), 1)
        record = records[0]
        self.assertTrue(record["malformed"])
        pin, pout = shim.price_for(shim.REQUESTED_MODEL)
        expected = record["tokens_in"] * pin + record["tokens_out"] * pout
        self.assertGreater(expected, 0.0, "the fixture must price above zero")
        self.assertEqual(
            record.get("cost"),
            {
                "cost_usd": expected,
                "tokens_in": record["tokens_in"],
                "tokens_out": record["tokens_out"],
            },
        )

    def test_a_replay_of_a_malformed_decision_still_fails_the_protocol(self):
        """The recorded cost must not turn the replay back into a usable
        decision: it stays a protocol fault."""
        shim = load_shim(self.tmp.name, max_calls="1")
        drive(shim, Client([Response("not json at all")]))
        replay = load_shim(self.tmp.name, max_calls="1")
        out = drive(replay, Client([]))
        self.assertIn("protocol_error", json.loads(out.strip()))
        self.assertEqual(replay.STATS["cache_hits"], 1)


class ModelPricingTests(CallCeilingCase):
    """A call this run cannot price is refused, never reported as free.

    `price_for` walked `PRICING` by prefix and returned `(0.0, 0.0)` when
    nothing matched, so a run on a model absent from the table reported every
    call as costing nothing and that zero was published as what the run spent.
    The prefix walk also billed a model whose name extends a priced one at the
    other model's card.
    """

    def test_an_unpriced_model_refuses_the_run_before_anything_is_dispatched(self):
        """The named cause is the missing rate card, and nothing else here can
        refuse this run.

        Four other causes could raise from `drive` and each is excluded rather
        than assumed:

          * the retry guard: the stand-in reports the compliant setting, and the
            control below is refused by nothing while reporting the same one;
          * a stand-in too thin to dispatch: the control drives the same client
            class to a decision;
          * the allowance: `LLM_MAX_CALLS` is 2 for one observation and the
            ledger is asserted empty afterwards;
          * the identity rule: the stand-in answers under the requested id, so
            it has nothing to refuse.

        The last assertion is what separates this refusal from every cause that
        lives further down: the client was never called, so the run stopped
        before the first observation was priced.
        """
        unpriced = "claude-not-a-model-9"
        shim = load_shim(self.tmp.name, max_calls="2", model=unpriced)
        client = Client([ReplyUnder(unpriced)])
        with self.assertRaises(shim.UnpricedModel) as caught:
            drive(shim, client)
        self.assertIn(f"no rate card for {unpriced}", str(caught.exception))
        self.assertEqual(client.requests, [], "the refusal precedes every dispatch")
        self.assertEqual(self.ledger_lines(shim), [], "and spends no unit")

    def test_the_control_is_a_priced_model_on_the_same_stand_in(self):
        """What makes the case above about pricing: the same client, the same
        allowance, the same observation, and a model the table prices runs to a
        decision."""
        shim = load_shim(self.tmp.name, max_calls="2")
        client = Client([Response('{"orders":[]}')])
        out = drive(shim, client)
        self.assertEqual(json.loads(out.strip())["orders"], [])
        self.assertEqual(len(client.requests), 1)
        self.assertGreater(shim.STATS["cost_usd"], 0.0)

    def test_a_name_that_extends_a_priced_model_is_not_billed_at_its_card(self):
        """The prefix defect in the accounting.

        `claude-opus-5-1` starts with `claude-opus-5` and is a different model.
        Under the prefix walk it was priced at Opus 5's card, silently. Only
        `price_for` can answer here, so nothing else needs excluding.
        """
        shim = load_shim(self.tmp.name)
        opus = shim.PRICING["claude-opus-5"]
        self.assertEqual(shim.price_for("claude-opus-5"), opus)
        self.assertEqual(shim.price_for("claude-opus-5-20260101"), opus)
        for extended in (
            "claude-opus-5-1",
            "claude-opus-5-1-20260101",
            "claude-opus-5-mini",
            "claude-opus-50",
        ):
            with self.subTest(model=extended):
                with self.assertRaises(shim.UnpricedModel):
                    shim.price_for(extended)

    def test_the_rate_card_is_matched_by_the_model_identity_rule(self):
        """One rule, not two, and one table, in `paper/evidence/llm_pricing.py`.

        `price_for` accepts a dated snapshot because `is_dated_snapshot_of` says
        it is the same policy, so narrowing that rule narrows the pricing match
        with it. Narrowing it in the shared module is also what shows this run
        prices from that module rather than from a copy of it: the assembler
        that publishes the cost reads the same file, so neither can price a
        model the other refuses. That the files carry no second copy is checked
        without the SDK in `paper/src/test_llm_pricing.py`.
        """
        shim = load_shim(self.tmp.name)
        pricing = sys.modules[shim.lookup_price.__module__]
        self.assertEqual(
            shim.price_for("claude-haiku-4-5-20251001"),
            pricing.PRICING["claude-haiku-4-5"],
        )
        self.assertIs(shim.PRICING, pricing.PRICING)
        digits = pricing.SNAPSHOT_DIGITS
        self.addCleanup(setattr, pricing, "SNAPSHOT_DIGITS", digits)
        pricing.SNAPSHOT_DIGITS = 6
        with self.assertRaises(
            shim.UnpricedModel,
            msg=(
                "pricing drift: the run priced a served id the shared rule no longer "
                "admits, so it is pricing from something other than "
                "paper/evidence/llm_pricing.py, which is what the assembler publishes "
                "from"
            ),
        ):
            shim.price_for("claude-haiku-4-5-20251001")

    def test_the_requested_model_the_field_runs_is_priced(self):
        """The table has to cover what the producer asks for, or the refusal
        above would stop the field itself."""
        shim = load_shim(self.tmp.name)
        self.assertGreater(shim.price_for(shim.REQUESTED_MODEL)[0], 0.0)


class AssemblerPricingTests(unittest.TestCase):
    """The same fail-open, on the script that publishes the number.

    `paper/evidence/assemble_llm_field.py` carried its own prefix walk and its
    own `(0.0, 0.0)` fallback, so a response cache naming a model its table does
    not price was assembled with `cost_usd` 0. It is a script rather than an
    importable module, so it is exercised the way an operator runs it: a copy in
    a temporary directory with a fabricated `final/` beside it.
    """

    ASSEMBLER = ROOT / "paper/evidence/assemble_llm_field.py"
    PRICING_MODULE = ROOT / "paper/evidence/llm_pricing.py"

    def assemble(self, cache_model):
        """Run the assembler over one response cache named for `cache_model`.

        The rate card travels with the script: it is the same
        `paper/evidence/llm_pricing.py` the shim prices by, imported from beside
        the assembler, so this fixture is the pair of files rather than one.
        """
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        script = Path(tmp.name) / "assemble_llm_field.py"
        script.write_text(self.ASSEMBLER.read_text(encoding="utf-8"), encoding="utf-8")
        (Path(tmp.name) / "llm_pricing.py").write_text(
            self.PRICING_MODULE.read_text(encoding="utf-8"), encoding="utf-8"
        )
        final = Path(tmp.name) / "final"
        final.mkdir()
        (final / "llm-field-records-all.jsonl").write_text(
            json.dumps({"agent_id": "llm-x", "model": cache_model, "dataset": "d"})
            + "\n",
            encoding="utf-8",
        )
        (final / f"llm-cache-{cache_model}.jsonl").write_text(
            json.dumps({"tokens_in": 10, "tokens_out": 5}) + "\n", encoding="utf-8"
        )
        done = subprocess.run(
            [sys.executable, str(script)], capture_output=True, text=True
        )
        return done.returncode, done.stdout + done.stderr

    def test_an_unpriced_model_stops_the_assembly(self):
        """Four other gates in that script can exit non-zero on this fixture:
        the empty-records check, the model set, the dataset set and the
        incompleteness check. Each of them states its own reason, so the
        assertion is on the pricing refusal's message rather than on the exit
        code, and the priced control below shows the fixture reaches those later
        gates when the model is one the table names.
        """
        code, output = self.assemble("claude-not-a-model-9")
        self.assertEqual(code, 1)
        self.assertIn("no rate card for claude-not-a-model-9", output)

    def test_a_priced_model_gets_past_pricing(self):
        code, output = self.assemble(MODEL)
        self.assertEqual(code, 1)
        self.assertNotIn("no rate card", output)
        self.assertIn("refusing to assemble: models", output)

    def test_a_name_that_extends_a_priced_model_is_not_billed_at_its_card(self):
        code, output = self.assemble("claude-opus-5-1")
        self.assertIn("no rate card for claude-opus-5-1", output)


class ProviderRequestTests(CallCeilingCase):
    """The unit is a provider request, not a dispatch from this process."""

    def test_the_client_the_run_uses_disables_the_sdk_automatic_retries(self):
        """The ceiling reserves once per `create`, so `create` must send once.

        Recorded at the constructor and driven through `main` with no client of
        its own, which is how the harness runs it. Asserting on
        `PROVIDER_MAX_RETRIES`, or calling `build_client` directly, would leave
        the run free to construct its client some other way.

        The named cause is the keyword the run passes its constructor, so the
        stand-in reports a compliant setting whatever it was constructed with.
        Otherwise the runtime guard refuses first and this case ends as an error
        raised inside `drive` rather than as its own assertion: dropping the
        keyword would then be caught by the guard, which two other cases already
        pin, and nothing here would say what the constructor was given.
        """
        shim = load_shim(self.tmp.name)
        seen = []

        def recording_constructor(**kw):
            seen.append(kw)
            client = Honouring(**kw)
            client.max_retries = shim.PROVIDER_MAX_RETRIES
            return client

        with unittest.mock.patch.object(
            shim.anthropic, "Anthropic", recording_constructor
        ):
            # No observations: the client is built, nothing is dispatched.
            drive(shim, None, observations=0)
        self.assertEqual(len(seen), 1, "the run builds exactly one client")
        self.assertEqual(seen[0].get("max_retries"), 0)

    def test_a_client_that_accepts_the_setting_and_ignores_it_refuses_the_run(self):
        """The failure the check exists for, not the happy path.

        A later SDK may keep taking `max_retries` and stop honouring it. The
        stand-in does exactly that: the keyword is accepted, the effective
        setting is two. Nothing may be dispatched under a client that will
        retry, so the run must refuse before the first observation.
        """
        shim = load_shim(self.tmp.name)
        with unittest.mock.patch.object(shim.anthropic, "Anthropic", Ignoring):
            with self.assertRaises(RuntimeError) as caught:
                drive(shim, None, observations=1)
        message = str(caught.exception)
        self.assertIn("max_retries=2", message)
        self.assertIn("Refusing to start", message)
        self.assertEqual(self.ledger_lines(shim), [], "nothing was reserved")

    def test_a_client_whose_setting_cannot_be_read_refuses_the_run(self):
        """"Cannot be determined" is a refusal, not an assumption.

        A client that has dropped the attribute entirely, which is what a rename
        or a move looks like from here, leaves the ceiling unprovable. The run
        must not proceed on the hope that it binds anyway.
        """
        shim = load_shim(self.tmp.name)
        with unittest.mock.patch.object(shim.anthropic, "Anthropic", Opaque):
            with self.assertRaises(RuntimeError) as caught:
                drive(shim, None, observations=1)
        message = str(caught.exception)
        self.assertIn("does not report a readable max_retries", message)
        self.assertEqual(self.ledger_lines(shim), [], "nothing was reserved")

    def test_build_client_refuses_to_return_a_client_that_would_retry(self):
        """The helper states a property of what it returns, so it is pinned.

        `main` checks every client it is given, which covers the run. It does
        not cover a caller that imports `build_client` and uses the client some
        other way, and it would leave the helper's own check unpinned, free to
        be deleted with the suite green. The stand-in constructor returns
        normally, so the refusal can only come from inside `build_client`.
        """
        shim = load_shim(self.tmp.name)
        with unittest.mock.patch.object(shim.anthropic, "Anthropic", Ignoring):
            with self.assertRaises(RuntimeError) as caught:
                shim.build_client()
        self.assertIn("max_retries=2", str(caught.exception))

    def test_a_client_the_caller_supplies_is_checked_like_one_the_run_builds(self):
        """The check is on the path to the provider, not on the constructor.

        `main` takes a client so the decision loop can be driven against a
        stand-in, and that parameter used to reach `client.messages.create` with
        the retry policy never read, which is the one path where the ceiling's
        stated guarantee did not hold.

        Four causes could make a refusal appear here without the check on this
        path, and each is excluded. A stand-in too thin to dispatch: `Ignoring`
        answers normally, and the control below drives the same shape of client
        to completion. A client the run built for itself after all: the
        constructor fails the test if it is called. An exhausted allowance: the
        ledger is asserted empty, and the message is the guard's. And the
        supplied-client path refusing whatever it is handed: the control is
        supplied the same way and is not refused.
        """
        shim = load_shim(self.tmp.name)

        def refuses_to_be_built(**kw):
            raise AssertionError("the run built a client although one was supplied")

        supplied = Ignoring()
        with unittest.mock.patch.object(
            shim.anthropic, "Anthropic", refuses_to_be_built
        ):
            with self.assertRaises(RuntimeError) as caught:
                drive(shim, supplied, observations=1)
        message = str(caught.exception)
        self.assertIn("max_retries=2", message)
        self.assertIn("Refusing to start", message)
        self.assertEqual(supplied.requests, [], "nothing was dispatched")
        self.assertEqual(self.ledger_lines(shim), [], "nothing was reserved")

        honouring = Honouring(max_retries=shim.PROVIDER_MAX_RETRIES)
        drive(shim, honouring, observations=1)
        self.assertEqual(len(honouring.requests), 1, "a compliant client dispatches")
        self.assertEqual(len(self.ledger_lines(shim)), 1)

    def test_n_units_allow_exactly_n_provider_requests_across_a_retryable_failure(self):
        """Two units, a 429 and an answer, and exactly two HTTP requests.

        A 429 is what the SDK retries on its own: under the default of two
        retries the first process alone would have sent three requests and
        billed three under one reserved unit. The client here is the real SDK
        over a stand-in transport, so the count is the SDK's behaviour.

        It is supplied to `main`, which is a path the runtime check covers: the
        client is built with the shim's own setting, so the check passes and the
        case runs through the guard rather than around it.
        """
        requests = []

        def transport(count_only_after):
            def handle(request):
                requests.append(request.url.path)
                if len(requests) <= count_only_after:
                    return httpx.Response(429, json={"type": "error"})
                return httpx.Response(200, json=message_payload('{"orders":[]}'))

            return httpx.Client(transport=httpx.MockTransport(handle))

        first = load_shim(self.tmp.name, max_calls="2")
        with self.assertRaises(RuntimeError) as caught:
            drive(first, sdk_client(first, transport(1)))
        self.assertIn("API failure", str(caught.exception))
        self.assertEqual(len(requests), 1, "the SDK must not retry the 429")
        self.assertEqual(len(self.ledger_lines(first)), 1)

        second = load_shim(self.tmp.name, max_calls="2")
        drive(second, sdk_client(second, transport(1)), observations=1, tag=1)
        self.assertEqual(len(requests), 2, "the respawn spends exactly one more")
        self.assertEqual(len(self.ledger_lines(second)), 2)

        third = load_shim(self.tmp.name, max_calls="2")
        with self.assertRaises(RuntimeError) as caught:
            drive(third, sdk_client(third, transport(1)), observations=1, tag=2)
        self.assertIn("budget exhausted", str(caught.exception))
        self.assertEqual(len(requests), 2, "a ceiling of two bought two requests")


if __name__ == "__main__":
    unittest.main()
