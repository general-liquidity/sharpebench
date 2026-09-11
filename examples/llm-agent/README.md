# Frontier-model field runner

`llm_agent.py` is the reproducible stdio adapter for SharpeBench's pending
frontier-model experiment. It is infrastructure, not a completed result.

The driver evaluates three explicitly named policies: Claude Fable 5, Claude
Opus 5, and Claude Haiku 4.5 as the small-model contrast. It requires the
Anthropic Python SDK and an API key with sufficient credit:

```bash
python -m pip install anthropic
export ANTHROPIC_API_KEY=...
cargo run --release -p sharpebench-harness --example llm_field_eval -- \
  paper/evidence/final/llm-field-records-all.jsonl
python paper/evidence/assemble_llm_field.py
```

The adapter caches each paid response under the whole effective request: the
model, the system prompt, the message, the token and thinking settings, and the
scaffold version that will interpret the reply. Provider errors, authentication
or credit failures, and an exhausted call budget terminate the subprocess.

`LLM_MAX_CALLS` is a ceiling on provider requests per model, not on cached
results. Each fresh call reserves one unit in `llm-attempts-<model>.jsonl`
beside the response cache, fsynced before the request is sent, so a call that
fails or times out still spends its unit and a respawned subprocess cannot
re-spend it. The client disables the SDK's own automatic retries, so one
reserved unit is exactly one HTTP request to the provider. That is checked, not
assumed: the effective retry setting is read back off the constructed client
before the run starts, and a client that reports a non-zero setting, or none
that can be read, refuses the run rather than risking several billable requests
per reserved unit. So the ceiling holds whatever SDK version is installed, or
the run does not start. Nothing is retried
inside the adapter: a rate limit or a timeout fails the subprocess, and the
harness respawn takes a fresh unit from the same ledger.

The Rust driver writes to a `.partial` file and publishes the
requested score file only after every model and dataset completes. The
assembler independently requires all three models, both datasets, and zero API
or budget errors before it can produce `llm-field.jsonl`.

No partial run is admissible as paper evidence.
