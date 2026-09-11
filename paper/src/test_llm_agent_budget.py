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


def load_shim(tmp, max_calls="1"):
    """Import the shim as a fresh module bound to a throwaway cache directory.

    Re-importing against the same directory is how a harness retry is modeled:
    a new process, the same model, the same cache and the same ledger.
    """
    argv = sys.argv
    environ = dict(os.environ)
    sys.argv = ["llm_agent.py", MODEL]
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


class Client:
    """A stand-in provider. Records every request and answers as instructed."""

    def __init__(self, answers):
        self.answers = list(answers)
        self.requests = []
        self.messages = self

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


class ProviderRequestTests(CallCeilingCase):
    """The unit is a provider request, not a dispatch from this process."""

    def test_the_client_the_run_uses_disables_the_sdk_automatic_retries(self):
        """The ceiling reserves once per `create`, so `create` must send once.

        Recorded at the constructor and driven through `main` with no client of
        its own, which is how the harness runs it. Asserting on
        `PROVIDER_MAX_RETRIES`, or calling `build_client` directly, would leave
        the run free to construct its client some other way.
        """
        shim = load_shim(self.tmp.name)
        seen = []

        def recording_constructor(**kw):
            seen.append(kw)
            return object()

        with unittest.mock.patch.object(
            shim.anthropic, "Anthropic", recording_constructor
        ):
            # No observations: the client is built, nothing is dispatched.
            drive(shim, None, observations=0)
        self.assertEqual(len(seen), 1, "the run builds exactly one client")
        self.assertEqual(seen[0].get("max_retries"), 0)

    def test_n_units_allow_exactly_n_provider_requests_across_a_retryable_failure(self):
        """Two units, a 429 and an answer, and exactly two HTTP requests.

        A 429 is what the SDK retries on its own: under the default of two
        retries the first process alone would have sent three requests and
        billed three under one reserved unit. The client here is the real SDK
        over a stand-in transport, so the count is the SDK's behaviour.
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
