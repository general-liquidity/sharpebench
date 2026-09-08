"""Model-identity and cache-identity regressions for the paid LLM shim.

The shim under test is `examples/llm-agent/llm_agent.py`, the producer behind
the LLM field. Nothing here calls a provider: the module is imported with a
temporary cache and statistics directory, and every API object is a stand-in.

Two findings are pinned:

  * a `NotFoundError` used to rebind the module-level model to an unversioned
    Haiku alias and retry, while the producer went on publishing the requested
    versioned id, so an alias-served policy could be reported and replayed as
    the model the field names;
  * the response-cache key was the SHA-256 of `model + "\\x00" + prompt` alone,
    so the system prompt, the token and thinking settings and the
    summarizer/parser version were all outside cache identity, and changing any
    of them silently replayed decisions taken under a different policy
    configuration.
"""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SHIM = ROOT / "examples/llm-agent/llm_agent.py"

MODEL = "claude-haiku-4-5-20251001"
FRONTIER = "claude-opus-5"


def load_shim(tmp, model=MODEL):
    """Import the shim as a fresh module bound to a throwaway cache directory.

    `sys.argv[1]` is the model id and the cache/statistics paths are resolved at
    import, so each case gets its own module object rather than sharing global
    state with the next one.
    """
    argv = sys.argv
    environ = dict(os.environ)
    sys.argv = ["llm_agent.py", model]
    os.environ["LLM_CACHE_DIR"] = str(tmp)
    os.environ["LLM_STATS_DIR"] = str(Path(tmp) / "stats")
    try:
        spec = importlib.util.spec_from_file_location(f"llm_agent_{id(tmp)}", SHIM)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    finally:
        sys.argv = argv
        os.environ.clear()
        os.environ.update(environ)


class Response:
    """The parts of an API response the shim reads."""

    def __init__(self, model):
        self.model = model
        self.content = []
        self.stop_reason = None


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


def not_found(shim):
    """The SDK's own unknown-model error, built the way the SDK builds it.

    A hand-rolled stand-in would not exercise the `except` clause the shim
    actually declares.
    """
    import httpx

    request = httpx.Request("POST", "https://api.anthropic.com/v1/messages")
    return shim.anthropic.NotFoundError(
        "model: not found",
        response=httpx.Response(404, request=request),
        body=None,
    )


class ShimCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.shim = load_shim(self.tmp.name)


class ModelIdentityTests(ShimCase):
    def test_no_fallback_model_is_configured(self):
        self.assertFalse(
            hasattr(self.shim, "HAIKU_FALLBACK"),
            "a configured fallback model is a second policy under one name",
        )
        self.assertNotIn("HAIKU_FALLBACK", SHIM.read_text(encoding="utf-8"))

    def test_an_unavailable_model_fails_the_run_instead_of_substituting(self):
        missing = not_found(self.shim)
        client = Client([missing, Response(self.shim.REQUESTED_MODEL)])
        with self.assertRaises(RuntimeError) as caught:
            self.shim.call_model(client, "prompt")
        self.assertIn(self.shim.REQUESTED_MODEL, str(caught.exception))
        self.assertEqual(
            len(client.requests), 1, "the shim must not retry under another model"
        )

    def test_the_requested_model_is_the_only_one_ever_requested(self):
        client = Client([Response(self.shim.REQUESTED_MODEL)])
        self.shim.call_model(client, "prompt")
        self.assertEqual(client.requests[0]["model"], self.shim.REQUESTED_MODEL)

    def test_a_substituted_served_model_is_refused(self):
        with self.assertRaises(RuntimeError) as caught:
            self.shim.effective_model(Response("claude-haiku-3-5-20241022"))
        self.assertIn("model substitution", str(caught.exception))

    def test_an_unversioned_alias_answering_a_versioned_request_is_refused(self):
        """The exact historical substitution: the versioned id was requested and
        the bare alias answered."""
        with self.assertRaises(RuntimeError):
            self.shim.effective_model(Response("claude-haiku-4-5"))

    def test_a_versioned_expansion_of_the_requested_alias_is_the_same_policy(self):
        alias = tempfile.TemporaryDirectory()
        self.addCleanup(alias.cleanup)
        shim = load_shim(alias.name, model="claude-haiku-4-5")
        served = shim.effective_model(Response("claude-haiku-4-5-20251001"))
        self.assertEqual(served, "claude-haiku-4-5-20251001")

    def test_both_identities_travel_with_every_cached_decision(self):
        key = self.shim.cache_key(self.shim.REQUESTED_MODEL, "prompt")
        cache = {}
        record = self.shim.record_decision(
            cache, key, self.shim.REQUESTED_MODEL, {"orders": []}
        )
        self.assertEqual(record["model_requested"], self.shim.REQUESTED_MODEL)
        self.assertEqual(record["model_effective"], self.shim.REQUESTED_MODEL)
        stored = json.loads(self.shim.CACHE_PATH.read_text(encoding="utf-8").strip())
        self.assertEqual(stored["model_requested"], self.shim.REQUESTED_MODEL)
        self.assertEqual(stored["model_effective"], self.shim.REQUESTED_MODEL)

    def test_the_run_reports_which_model_answered(self):
        self.assertIn("model_requested", self.shim.STATS)
        self.assertIn("model_effective", self.shim.STATS)
        self.assertIsNone(
            self.shim.STATS["model_effective"],
            "an unanswered run must not claim an effective identity",
        )


class CacheIdentityTests(ShimCase):
    def key(self, prompt="prompt"):
        return self.shim.cache_key(self.shim.REQUESTED_MODEL, prompt)

    def test_the_system_prompt_participates_in_cache_identity(self):
        before = self.key()
        self.shim.SYSTEM = self.shim.SYSTEM + " Prefer cash."
        self.assertNotEqual(before, self.key())

    def test_the_scaffold_version_participates_in_cache_identity(self):
        before = self.key()
        self.shim.SCAFFOLD_VERSION = "summarize-v2/parse-v1"
        self.assertNotEqual(before, self.key())

    def test_the_token_and_sampling_settings_participate_in_cache_identity(self):
        """Haiku is asked at temperature 0 and 300 tokens, the frontier tier at
        4000 tokens with adaptive thinking. Those are different policies, and
        the settings are what separates them, so the settings themselves must
        move the key rather than only the model name that happens to select
        them."""
        frontier = tempfile.TemporaryDirectory()
        self.addCleanup(frontier.cleanup)
        other = load_shim(frontier.name, model=FRONTIER)
        self.assertNotEqual(
            self.shim.request_kwargs(MODEL, "p")["max_tokens"],
            other.request_kwargs(FRONTIER, "p")["max_tokens"],
        )
        before = self.key()
        base = self.shim.request_kwargs

        def widened(model, prompt):
            kw = base(model, prompt)
            kw["max_tokens"] = kw["max_tokens"] * 2
            return kw

        self.shim.request_kwargs = widened
        self.assertNotEqual(before, self.key())

    def test_the_prompt_still_participates_in_cache_identity(self):
        self.assertNotEqual(self.key("a"), self.key("b"))

    def test_an_unchanged_configuration_still_hits_the_cache(self):
        """Strictness must not defeat the point of the cache: the same request
        under the same scaffold is reused, which is what makes a recorded run
        replayable for free."""
        key = self.key()
        self.shim.record_decision(
            {}, key, self.shim.REQUESTED_MODEL, {"orders": [], "cost": None}
        )
        reloaded = self.shim.load_cache()
        self.assertIn(key, reloaded)
        self.assertEqual(self.shim.STATS["cache_records_ignored"], 0)

    def test_a_decision_from_another_scaffold_version_is_not_replayed(self):
        key = self.key()
        self.shim.record_decision({}, key, self.shim.REQUESTED_MODEL, {"orders": []})
        self.shim.SCAFFOLD_VERSION = "summarize-v2/parse-v1"
        self.assertEqual(self.shim.load_cache(), {})
        self.assertEqual(self.shim.STATS["cache_records_ignored"], 1)

    def test_a_record_whose_digest_is_not_its_key_is_not_replayed(self):
        """The stored digest is what lets a replay check the record against the
        configuration it claims, so a record that disagrees with itself is
        dropped rather than trusted."""
        self.shim.CACHE_PATH.parent.mkdir(parents=True, exist_ok=True)
        self.shim.CACHE_PATH.write_text(
            json.dumps(
                {
                    "key": self.key(),
                    "request_sha256": "0" * 64,
                    "scaffold_version": self.shim.SCAFFOLD_VERSION,
                    "model_requested": self.shim.REQUESTED_MODEL,
                    "orders": [],
                }
            )
            + chr(10),
            encoding="utf-8",
        )
        self.assertEqual(self.shim.load_cache(), {})
        self.assertEqual(self.shim.STATS["cache_records_ignored"], 1)

    def test_an_unreadable_record_is_counted_not_silently_skipped(self):
        self.shim.CACHE_PATH.parent.mkdir(parents=True, exist_ok=True)
        self.shim.CACHE_PATH.write_text("{ truncated" + chr(10), encoding="utf-8")
        self.assertEqual(self.shim.load_cache(), {})
        self.assertEqual(self.shim.STATS["cache_records_ignored"], 1)


if __name__ == "__main__":
    unittest.main()
