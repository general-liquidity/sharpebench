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

The adapter caches each paid response by model and prompt. Provider errors,
authentication or credit failures, and an exhausted call budget terminate the
subprocess. The Rust driver writes to a `.partial` file and publishes the
requested score file only after every model and dataset completes. The
assembler independently requires all three models, both datasets, and zero API
or budget errors before it can produce `llm-field.jsonl`.

No partial run is admissible as paper evidence.
