# Local-agent build audit — 0a55970 / v0.11.0

Date: 2026-08-26. Scope: commit `0a55970` "feat(local-agents): harden the sandbox
and field contract" plus release `5838605` v0.11.0, audited against the build
list. Method: read the code, run it. Commit messages, docstrings and CHANGELOG
entries were treated as claims to be checked, not as evidence.

Machine: Windows 11, Docker daemon **not running** (`docker version` fails to
connect to `npipe:////./pipe/dockerDesktopLinuxEngine`). That materially limits
what could be verified live for the sandbox — flagged where it bites.

## Commands run

```
cargo test --release --workspace
  -> exit 0.  31 test binaries.  494 passed, 0 failed, 0 ignored, 0 warnings.

cargo clippy --all-targets --workspace -- -D warnings
  -> exit 0.  No warnings.

cargo run --release -p sharpebench-harness --example external_rules_eval -- <tmp>
  -> exit 0, 351 records, ~130s.
     sha256 14fd1306755457d3e16162ec983eca53e7b69f6ed000fc6e9a49875589c2a9af
     BYTE IDENTICAL to paper/evidence/final/external-rules.jsonl

cargo run --release -p sharpebench-harness --example evidence_sweep -- <tmp> us-indices-1d
  -> exit 0, 512 records.
     BYTE IDENTICAL to paper/evidence/final/us-indices-1d.jsonl

./target/release/sharpebench.exe sandbox-check ...           (three invocations, below)
latexmk -pdf -interaction=nonstopmode -halt-on-error main.tex  -> exit 0, 48 pages
```

---

## Item-by-item verdicts

### 1. Sandbox hardening — **BUILT**

The refusal-to-fall-back property still holds, and it is now stronger than the
`docker run --rm --network none --memory 1g --cpus 1 -i` baseline described in
the build list.

The decision is isolated in a pure function so it is testable without Docker
(`crates/sharpebench-arena/src/sandbox.rs:110-141`):

```rust
if docker_present {
    validate_image(image, opts.allow_unpinned_image)?;
    return Ok(Launch::Docker { program: "docker".to_string(), args: hardened_docker_args(image) });
}
if !opts.allow_unsandboxed {
    return Err(SandboxError::DockerUnavailable( ... ));
}
match &opts.unsandboxed_command {
    Some(cmd) if !cmd.is_empty() => Ok(Launch::Unsandboxed { ... }),
    _ => Err(SandboxError::DockerUnavailable(
        "allow_unsandboxed is set but no unsandboxed_command was supplied; refusing to guess a host command")),
}
```

Both conditions are still required (`allow_unsandboxed` **and** a non-empty
explicit command), `SandboxOptions` still derives `Default` with
`allow_unsandboxed: false` (`sandbox.rs:32-45`), and Docker-present always wins
over the opt-in (`sandbox.rs:410-462` pins that). `run_external_sandboxed`
(`sandbox.rs:353-365`) is the only production entry point and it routes through
`resolve_launch(docker_available(), ...)` — there is no second spawn path inside
the arena crate. Grep for `ExternalAgent::spawn` across the workspace confirms
the only other callers are the CLI `run` command, the two harness examples and
sim's own tests — none of which claim to sandbox.

New in this commit, verified by reading:

- `--pull never` (`sandbox.rs:147-149`), so a missing image errors rather than
  silently fetching one.
- Digest pinning by default: `validate_image` (`sandbox.rs:318-348`) requires
  `<repository>@sha256:<64 lowercase hex>` unless `allow_unpinned_image` is set,
  and rejects empty / whitespace / `-`-prefixed values so the image argument can
  never be smuggled in as a `docker run` flag.
- `--cap-drop ALL`, `no-new-privileges=true`, `--user 65532:65532`,
  `--read-only`, `--ipc none`, `--pids-limit 128`, `nofile=256:256`,
  `noexec,nosuid,nodev` tmpfs, `--log-driver none` (`sandbox.rs:143-183`).
- `check_sandbox_readiness` (`sandbox.rs:250-316`) runs a hostile probe
  (`HOSTILE_PROBE`, `sandbox.rs:187-200`) *inside the same* `hardened_docker_args`
  boundary, asserting uid 65532, `CapEff` all-zero, `NoNewPrivs=1`, loopback-only
  netns, read-only `/`, and that a chmod +x file on `/tmp` will not execute.
  It requires the image locally present and never pulls.

CLI behaviour verified live (Docker down):

```
$ sharpebench sandbox-check
usage: sharpebench sandbox-check <fixture@sha256:digest> [--json]   exit=2

$ sharpebench sandbox-check alpine:latest
NOT FIELD-READY — invalid sandbox config: field images must be pinned as
<repository>@sha256:<64 lowercase hex>...                            exit=1

$ sharpebench sandbox-check alpine@sha256:aaaa...(64) --json
{ "error": "docker unavailable: failed to connect to the docker API ...",
  "field_ready": false }                                             exit=1
```

Fails closed in all three. No path degraded silently.

**Caveat, not a defect in the code but a defect in the evidence:** none of the
live boundary is exercised anywhere in this audit or in CI on this machine.
`check_sandbox_readiness` has exactly one test
(`sandbox.rs:485-490`) and it only asserts that an unpinned image is rejected
*before* Docker is contacted. The hostile probe itself has never been observed
to pass. See Finding F-1.

### 2. Protocol field contract (+105) — **BUILT, but NOT additive; the CHANGELOG mislabels it**

What was added:

- `#[serde(deny_unknown_fields)]` on all six wire types: `MarketObservation`
  (`crates/sharpebench-protocol/src/lib.rs:18`), `SymbolSnapshot` (`:29`),
  `PositionState` (`:44`), `Decision` (`:53`), `DecisionCost` (`:72`),
  `Order` (`:108`).
- `Decision::validate_for(&MarketObservation)` (`lib.rs:137-183`): point-in-time
  symbol membership, one order per symbol, finite `target_weight` with
  `abs() <= 1.0`, finite `confidence` in `[0,1]`, finite nonnegative
  `cost.cost_usd`.
- A doc correction on `Order::target_weight` from "in [0, 1] (signed for shorts)"
  — which was self-contradictory — to "in [-1, 1]; negative values are shorts".
- One test, `closed_decision_contract_rejects_drift_and_semantic_faults`
  (`lib.rs:356-403`), which is real: it asserts on unknown-field rejection and on
  three distinct semantic faults.

**This is a breaking change to the entrant contract, shipped as `### Fixed` in a
minor release.** Before 0a55970 a third-party agent could emit
`{"orders":[...], "reasoning":"...", "latency_ms": 12}` and be scored. After it,
that same agent deserializes to `DecideError::Protocol`, which
`external.rs` classifies as `FailureKind::AgentProtocolViolation` — a
non-retryable **agent fault** that is materialised as a sentinel failing run
(`crates/sharpebench-harness/src/failure.rs:38-41, 63-65`). A previously
conforming agent silently starts scoring as broken. The CHANGELOG entry
(`CHANGELOG.md`, Unreleased/Fixed) calls this "the closed `Decision` contract
rejects unknown fields" without saying that entrants must change. Nothing in the
diff bumps a protocol version or provides a compatibility window. Contrast with
how the project handled the analogous case one release earlier, where
`long_only_beta` was explicitly kept as "a compatibility alias".

There is **no published JSON schema in this repository** — the only
`additionalProperties` hits are inside `npm/mcp/node_modules/ajv`. So the exact
SharpeArena bug (type has a field, schema forbids it) cannot occur here, because
there is no second artifact to drift. But the underlying risk has been imported
into the Rust type itself rather than eliminated.

Coverage is also asymmetric: `DecisionStep` (`lib.rs:191`), `RunTrajectory`
(`:207`), `DeclaredMandate` (`:233`) and `AgentTrajectory` (`:259`) did **not**
get `deny_unknown_fields`. Whether deliberate or not, it is undocumented — the
module now has two contract disciplines with no stated rule for which applies
where.

`MarketObservation` carrying `deny_unknown_fields` is the least defensible of
the six: the harness only ever *serializes* observations, so the attribute buys
nothing today, while permanently making the observation format non-extensible
for anyone who deserializes it.

### 3. `local_open_weight_field_eval.rs` — **PARTIAL / NOT-VERIFIABLE here**

It is real code, not a scaffold with a stub agent: 359 lines that build the same
reference field (`buy-and-hold`, `momentum`, `hold` + 5 luck-floor agents,
`local_open_weight_field_eval.rs:158-196`), run the same `walk_forward` windows,
the same `EXEC_SEEDS`, the same `rank()` with a `never_catastrophic` ablation,
and write the same record shape as `llm_field_eval`. There is no fake agent and
no constant-returning stub anywhere in it. Output is written to `{out}.partial`
and only `std::fs::rename`d on full success (`:223-226, :356-357`), so a partial
field is never published under the real name — that is well done.

But **it does not run a real local model in this repository, and cannot.** It
spawns `python -m sharpearena.ollama_shim` (`:250-263`), a module that lives in
the *sibling SharpeArena repo*:

```
$ python -c "import sharpearena.ollama_shim"
ModuleNotFoundError: No module named 'sharpearena'
$ find . -name "*ollama*"     # nothing in this repo
```

So every part of the actual model interaction — prompt construction, Ollama HTTP
call, thinking-mode handling, sampling, identity capture, malformed-output
emission — is out of tree and was not audited. Nothing in this repo tests it. It
has **zero tests**: no `#[test]` in the file, and no integration test drives it.

If Ollama is not running: the shim (out of tree) presumably fails; on this side,
`ExternalAgent::spawn(&python, &arg_refs).ok()` (`:277`) yields `None` on a spawn
failure, which `run_external_agent` maps to `FailureKind::SpawnError`
(`crates/sharpebench-harness/src/lib.rs:259-262`), a runtime failure, which trips
`result.failures.runtime_failures() > 0` and **panics** with "refusing to publish
partial evidence" (`:282-287`). That is correct fail-closed behaviour. But it is
inferred from reading, not observed — Ollama is not installed here.

Evidence shape: yes, it emits the committed JSONL shape plus five new keys
(`model_identity`, `decision_cadence`, `thinking`, `agent_protocol_failures`,
`asset_class`). No committed artifact exists for it, which the paper is honest
about (see item 8).

The CHANGELOG claim "The path is built and tested" is **not supported**. Built,
yes. Tested, no — by any reading of "tested" that survives `grep -c '#\[test\]'`
returning 0 for that file.

The README example uses the tag `qwen3.8:27b`
(`README.md`, SharpeArena composition section). No such Ollama tag exists —
Qwen3 has no `3.8` line and no `27b` size (27B is Gemma). This looks like an
invented placeholder in operator-facing copy. See Finding F-4.

### 4. Model identity in evidence — **PARTIAL**

Recorded (`ModelIdentity`, `local_open_weight_field_eval.rs:56-74`, embedded in
`Record.model_identity` at `:95`): `model`, `digest`, `parameter_size`,
`quantization`, `family`, `context_length`, `server`, `server_version`,
`size_bytes`, `format`, `capabilities`, and four content hashes
(`license_sha256`, `modelfile_sha256`, `template_sha256`, `parameters_sha256`).
Also on the record: `decision_cadence` (`:96`) and `thinking` (`:97`). That is a
genuinely good identity set — digest and quantization are the two that usually
go missing, and both are here.

What is **not** recorded, and should be for the reproducibility claim to hold:

- **`max_tokens`.** Read from `SHARPEBENCH_LOCAL_MAX_TOKENS` (`:218`), passed to
  the shim (`:256-257`), and then never written to any `Record` field. A field
  entry cannot be reproduced without it.
- **`timeout_seconds`** (`:219`) — same: used, not recorded. It bounds decisions
  and therefore can change results.
- **Temperature, top-p, top-k, sampling seed.** There is no flag for any of them
  anywhere in this file. Sampling is whatever the out-of-tree shim and Ollama
  default to. `parameters_sha256` *may* cover an Ollama Modelfile `PARAMETER`
  block, but it is `Option<String>`, it is opaque (a hash, not attributable
  values), and it would not capture a per-request override. For an LLM field
  eval, unrecorded temperature is the single biggest reproducibility hole.

Attribution has a second weakness: the identity is **self-reported by the shim**.
SharpeBench writes `--identity-out <path>` and then simply reads back whatever
the shim wrote (`read_identity`, `:198-203`). The evaluator never queries Ollama
itself and never cross-checks the digest. The independence claim in the README
("SharpeArena is the environment; SharpeBench is the evaluator") is weakened by
the evaluator taking the subject's word for the subject's identity. It does
fail closed if the file is absent or invalid (`panic!` at `:200-202`), which is
the right default.

### 5. Transport integrity — **BUILT**

The +50 lines did not weaken the no-silent-hold property; they tightened it.

The single change to the decision path is that both transports now go through
one parser (`crates/sharpebench-sim/src/external.rs:67-76`):

```rust
fn parse_decision(response: &str, observation: &MarketObservation) -> Result<Decision, DecideError> {
    let decision: Decision = serde_json::from_str(response).map_err(|_| DecideError::Protocol)?;
    decision.validate_for(observation).map_err(|_| DecideError::Protocol)?;
    Ok(decision)
}
```

called from stdio (`:154`) and HTTP (`:278`), replacing two bare
`serde_json::from_str(...).map_err(|_| DecideError::Protocol)` calls. Semantic
faults therefore return the **same** typed `DecideError::Protocol` that shape
faults already did, so they flow through the identical existing machinery:
`decide_with_retry` -> `CircuitBreaker` -> `TransportHealth` ->
`FailureKind::AgentProtocolViolation` -> `RunOutcome::AgentFault` -> sentinel
failing run counted against pass^k
(`crates/sharpebench-harness/src/failure.rs:38-41, 60-65, 96-99`). Nothing
routes around the flagging. `error_hold` (`external.rs:50-56`) is unchanged and
its contract ("the health, not this value, carries whether it was a masked
fault") still holds.

The new local-model path routes through the same place: it uses
`ExternalAgent` unmodified (`local_open_weight_field_eval.rs:277`) and reads
`result.failures.agent_faults()` / `.runtime_failures()` (`:282, :288`) rather
than inventing its own accounting. No bypass.

The complementary change on the Python side is the right one and is the part
that closes the loop end to end: `llm_agent.py` previously converted malformed
model output into `hold("malformed model output -> hold", cost)`; it now emits
`{"protocol_error": "malformed model output"}` (diff at
`examples/llm-agent/llm_agent.py`), which fails the wire contract deliberately
so the Rust side records an agent fault. The cached-replay branch does the same
(`if cache[key].get("malformed"): decision = {"protocol_error": ...}`) with a
comment explaining that replaying a cached malformed response must not resurrect
the old masked-hold behaviour. That is exactly the failure mode the build list
was worried about, and it was handled deliberately.

Also tightened, correctly: `parse_decision` in Python no longer clamps
(`w = min(max(w, 0.0), 1.0)` -> reject non-finite / out-of-range) and no longer
silently normalises an over-allocated book (`total > 1.0` now returns `None`
instead of rescaling). Both were quiet result-changing repairs; both are gone.

One test covers it and it asserts something real
(`external_wire_parser_fails_closed_on_semantic_faults`, `external.rs:325-345`):
four distinct invalid payloads, each asserted to match `Err(DecideError::Protocol)`.

`llm_field_eval.rs` was updated to match: the abort condition changed from
`!res.failures.is_empty()` to `res.failures.runtime_failures() > 0`, so
model-output faults become scored evidence instead of aborting the field, while
infrastructure faults still abort. That is the correct split.

### 6. `engine.rs` +85 — **BUILT; goldens verified to still reproduce**

Two changes.

(a) The clamp is gone (`crates/sharpebench-sim/src/engine.rs:166-173`):

```rust
-    let target_value = target_weight.max(0.0) * cur_nav;
+    let target_value = target_weight * cur_nav;
```

This is a real semantic change to the stepping engine: a negative target used to
mean "go flat", and now means "open a short". Any committed run in which an agent
emitted a negative weight would move. Verified none do:
`grep -c '"target_weight":-' paper/evidence/final/*.jsonl` returns 0 for every
file, and every shipped reference agent plus the Python `llm_agent.py`
`parse_decision` (which requires `0.0 <= w <= 1.0`) is long-only by construction.

(b) Financing now charges on gross rather than net exposure, gated on a new
`Book.has_short_target` flag (`engine.rs:61-67, 308-311, 356-375`). The gate is
explicitly a golden-preservation device and is honestly commented as such:

```rust
} else {
    // Preserve the historical long-only arithmetic exactly.  Small
    // execution-price overshoots around a zero target existed in the
    // frozen engine before signed shorts were supported; treating those
    // as intentional shorts would move every committed golden.
    positions_value / nav_now
}
```

I dislike carrying two financing formulas selected by run history, and it should
be revisited once the long-only goldens are re-frozen — but the reasoning is
sound, the alternative is worse, and it is documented at the point of the hack
rather than in a commit message. The flag is `#[serde(default, skip_serializing_if = "is_false")]`
so `true` still round-trips through a serialized `Book`; a resumed run does not
silently lose it.

**Determinism and goldens: verified empirically, not taken on trust.** Both
producers I could run reproduce byte-identically at HEAD:

| artifact | records | result |
|---|---|---|
| `paper/evidence/final/external-rules.jsonl` | 351 | `cmp` clean, sha256 `14fd1306…2c9af` |
| `paper/evidence/final/us-indices-1d.jsonl` | 512 | `cmp` clean |

That is the strongest single result in this audit: the engine change is
golden-neutral in fact, not just in claim.

The new test `signed_targets_open_shorts_and_financing_uses_gross_exposure`
(`engine.rs:920-966`) asserts three non-trivial things — long leg opens, short
leg goes negative, and a 2x-gross/0x-net book pays exactly one 100bp carry
(`(outcome.ret + 0.01).abs() < 1e-12`). That last assertion is a real numeric
check, not a tautology.

Note for the record: `CONCENTRATION_CAP` (`engine.rs:15`, 0.5) only pushes a
`ProcessEvent::ConcentrationBreach` trace event — it is not a hard cap. With
signed targets, gross exposure is now bounded only by `n_symbols * 1.0` via
`validate_for`'s per-order `abs() <= 1.0`. Unbounded gross was already reachable
on the long side before this commit, so this is not a regression, but the short
side doubles the reachable range and nothing gates it beyond the financing charge.

### 7. CLI additions (+128) — **BUILT, thinly tested**

Two additions.

- `sharpebench sandbox-check <image@sha256:digest> [--json]`
  (`crates/sharpebench-cli/src/main.rs:144-176`), dispatched at `:48`, listed in
  `help()` at `:626`. Verified running (three invocations above): correct usage
  message and exit 2 on missing arg, exit 1 and a typed error on an unpinned
  image, exit 1 and structured JSON `{"field_ready": false, "error": ...}` when
  Docker is unreachable.
- `sharpebench score --periods-per-year N --execution-seeds-per-window N`
  (`:1502-1518`), with validating parsers `positive_f64_flag` / `positive_usize_flag`
  (`:1530-1550`), and the usage string updated at `:34`.

Checked for a silent default change: `run_score` now builds
`ScoreConfig::for_periods_per_year(ScoreConfig::default().periods_per_year)`
instead of `ScoreConfig::default()`. These are identical —
`for_periods_per_year` is `Self { periods_per_year, ..Self::default() }`
(`crates/sharpebench-core/src/composite.rs:678-683`). No behaviour drift.

Tests: two, both new (`main.rs:1627-1675`), and both only exercise the two flag
*parsers* in isolation. `run_sandbox_check` itself has no test, and neither does
the `score` wiring — nothing asserts that `--periods-per-year 8760` actually
reaches `ScoreConfig`. The parsers are the easy part; the wiring is where a
regression would live.

Documentation: `sandbox-check` is in `sharpebench --help` but appears **nowhere**
in `paper/`, `docs/` or `README.md` (`grep -rn "sandbox-check" paper/ docs/ README.md`
returns nothing). The `score` flags are in the usage string only.

### 8. Paper Appendix A — **PARTIAL**

`local_open_weight_field_eval` **is** documented (`paper/sections/A-commands.tex`,
new paragraph), and the framing is the most intellectually honest thing in this
commit:

> \paragraph{The local open-weight compatibility path (unrun).}
> … This command is listed to make the built boundary inspectable. It has not
> produced an evidence file admitted by this manuscript and supports no result above.

Labelling your own new feature "(unrun)" in the appendix, and stating outright
that it supports no result, is the correct call and it should be said plainly:
this is well done.

`sandbox-check` is **not** in Appendix A, or anywhere else in the paper. That is
the one new user-facing entry point in this commit that a reader could actually
run to check a claim the paper makes about the arena boundary, and it is absent.

Commands as written: the `external_rules_eval` invocation
(`A-commands.tex:66-70`) runs verbatim and reproduces its artifact byte-for-byte
(above). The `local_open_weight_field_eval` invocation cannot run without
SharpeArena installed — which the paragraph does not say. It says "unrun", not
"unrunnable from this repository". A reader who installs Ollama and follows the
appendix hits `ModuleNotFoundError: No module named 'sharpearena'`. The
prerequisite belongs in the paragraph.

Paper build (`latexmk -pdf`, TeX Live 2026, built to a scratch `-outdir` so the
tree was not mutated):

- exit 0, 48 pages
- **0 Overfull boxes**
- 22 Underfull boxes (all vbox, cosmetic)
- **0 undefined references, 0 undefined citations, 0 multiply-defined labels**

Clean. Note the rebuilt PDF is not byte-identical to `paper/main.pdf`
(`16e54447…` vs `9d0682a3…`) though it is the same 610156 bytes — ordinary
PDF timestamp nondeterminism, not a content difference. No `SOURCE_DATE_EPOCH`
pinning is configured.

---

## Separate findings

### F-1. The live sandbox boundary has never been observed to hold. **The one test that would prove it silently skips.**

`sandbox.rs:516-534`:

```rust
#[test]
fn docker_spawn_smoke() {
    if !docker_available() {
        eprintln!("SKIP docker_spawn_smoke: docker is not available on this machine");
        return;
    }
    ...
    let decision = agent.decide(&obs);
    assert!(decision.orders.is_empty());
}
```

This is the only test in the workspace that touches a real container, and on any
machine without a running Docker daemon — including this one, and including any
CI runner without Docker — it returns green having asserted nothing. `cargo test`
does not surface the `eprintln!` unless the test fails, so the skip is invisible
in the run I performed: the suite reports 494 passed / 0 ignored, and one of
those 494 "passes" is a no-op. The comment "so CI without Docker stays green and
honest" describes the intent, but the mechanism delivers green without
delivering honest — an operator reading the summary cannot tell.

Worse, the assertion is weak even when it *does* run:
`assert!(decision.orders.is_empty())` passes for essentially any failure mode,
since `error_hold` returns empty orders. It confirms "did not hang or panic",
which is worth something, but it does not confirm the sandbox flags took effect.

`check_sandbox_readiness` — the function that *would* prove the boundary, with a
genuinely well-designed hostile probe — has one test
(`field_readiness_requires_a_pinned_fixture_before_contacting_docker`,
`:485-490`) that only checks the pre-Docker validation path. The `HOSTILE_PROBE`
shell script has never been executed by anything in this repository.

The second test that touches it,
`hostile_probe_runs_inside_the_same_hardened_boundary` (`:493-511`), asserts on
an argument vector it constructs itself two lines earlier. It is a useful
regression pin against someone building the probe with weaker flags, but it is
string comparison, not a security property.

**Recommendation:** make Docker a hard requirement in at least one CI job and
call `check_sandbox_readiness` there against a pinned fixture. Until then, every
statement in the README, CHANGELOG and paper about the container boundary is
unverified by any executed code. Alternatively use `#[ignore]` plus an explicit
CI invocation, so a skip is visible in the test summary instead of counting as a
pass.

### F-2. Stray untracked `external-rules.jsonl` at the repo root — confirmed stray, CWD-relative default

The untracked root file is **byte-identical** to `paper/evidence/final/external-rules.jsonl`
(both sha256 `14fd1306755457d3e16162ec983eca53e7b69f6ed000fc6e9a49875589c2a9af`,
both 351 lines). It is a stray artifact, not a divergent copy — no data at risk,
safe to delete.

Cause (`crates/sharpebench-harness/examples/external_rules_eval.rs:416-418`):

```rust
let out = env::args().nth(1).unwrap_or_else(|| "external-rules.jsonl".to_string());
let mut w = BufWriter::new(File::create(&out).expect("create output"));
```

The default is a bare filename, resolved against the process CWD. The Appendix A
invocation passes an explicit `paper/evidence/final/external-rules.jsonl`, so
someone ran the example without the argument — likely a smoke run — and it landed
at the repo root.

Whether to "fix" it: the code is not wrong (an explicit path is documented and
works), but the failure mode is that a bare run overwrites nothing and quietly
produces a second copy of a paper artifact in an unversioned location, which is
exactly how a wrong file gets committed later. The same pattern is in every
example: `local_open_weight_field_eval.rs:206-208`
(`"local-open-weight-field.jsonl"`), and Appendix A's `pass_witness` invocation
writes `pass-witness.jsonl` to CWD. The cheap fix is to require the output path
as a positional argument (no default) in the evidence producers, so a run that
does not say where the evidence goes fails instead of scattering it. Add
`/external-rules.jsonl` and siblings to `.gitignore` at minimum.

### F-3. `agent_protocol_failures` is emitted unconditionally, so `llm-field-records-all.jsonl` no longer reproduces

`llm_field_eval.rs:76-80` adds `agent_protocol_failures: usize` with **no**
`#[serde(skip_serializing_if = ...)]`, unlike the neighbouring `model` field.
The committed artifact predates it:

```
$ head -1 paper/evidence/final/llm-field-records-all.jsonl | python -c "... print(sorted(d.keys()))"
[... 'agent_id', 'asset_class', ..., 'worst_run_drawdown']    # no agent_protocol_failures
```

So re-running `llm_field_eval` at HEAD emits a strictly different record schema
than the committed evidence. That artifact is now unreproducible by definition,
independent of any API-key or cost consideration. The paper describes the paid
LLM artifacts as incomplete and excluded from provenance, so nothing published
rests on it — but the file is still in `paper/evidence/final/` and the divergence
is undocumented. Either add `skip_serializing_if = "is_zero"` to preserve the old
shape, or regenerate, or move the stale artifact out of `final/`.

### F-4. `qwen3.8:27b` in the README is not a real model tag

`README.md`, SharpeArena composition section:

```bash
export SHARPEBENCH_LOCAL_MODELS='qwen3.8:27b'
```

Qwen3 has no `3.8` release line and no 27B parameter size (27B is Gemma 2/3).
The example is copy-pasteable and will fail. Since `SHARPEBENCH_LOCAL_MODELS`
is documented as taking *exact* Ollama tags — and the whole point of the identity
recording is exactness — shipping an invented tag in the one worked example
undercuts the feature. Replace with a tag that exists (`qwen3:8b`,
`gemma3:27b`, whatever was actually run).

### F-5. CHANGELOG says "built and tested"; there are no tests

`CHANGELOG.md`, Unreleased/Added:

> harness: `local_open_weight_field_eval` … The path is built and tested; no
> model performance result is admitted yet.

`crates/sharpebench-harness/examples/local_open_weight_field_eval.rs` contains
zero `#[test]` functions and no integration test drives it. The 359 new lines
are covered only by `cargo build` and clippy. The second clause of that sentence
is scrupulously honest; the first is not.

### F-6. Provenance manifest is 4 commits stale, including the sim and protocol

`paper/evidence/provenance.json` pins `repository_base_head: f62f4f89` and
`source_snapshot_sha256: 8fc377d4…`, both quoted in `A-commands.tex:12`.
Recomputing the 144 listed file hashes at HEAD: **43 of 144 changed, 0 missing.**
Among them `crates/sharpebench-sim/src/engine.rs`,
`crates/sharpebench-protocol/src/lib.rs`, `crates/sharpebench-sim/src/external.rs`,
`crates/sharpebench-core/src/composite.rs` — i.e. the simulation and scoring path
itself moved after the snapshot that the paper's headline integrity hash covers.

This is not dishonest — the manifest names its base commit, and the paper says
the external-rules file is explicitly *not* covered by it. And I verified
empirically that the two reproducible goldens still match at HEAD, which is the
substantive check. But two things follow:

1. There is no tool in the repo to recompute or verify `source_snapshot_sha256`.
   `reproduction_entrypoint` is the prose string `"commands in
   paper/sections/A-commands.tex"`. A reader has to write the checker I wrote.
   For a paper whose thesis is verifiable evaluation, `sharpebench verify-provenance`
   is a conspicuous absence.
2. The manifest should be regenerated at the release commit, or the paper should
   say explicitly that the hash describes a historical snapshot and that the
   scorer has since changed.

### F-7. No placeholders, TODOs or fake constants found

Swept `crates/`, `examples/`, `xtask/`, `scripts/` for
`todo!|unimplemented!|FIXME|placeholder|for now|not implemented|stub`. The single
hit is a deliberate scope statement, not a stub:

```
crates/sharpebench-core/src/regime_compare.rs:39:
//! **Deliberately NOT implemented:** a GAMLSS fit. There are no link functions,
```

No hardcoded fake values in the scoring or engine path. No test found asserting
nothing meaningful, with the two exceptions named above (F-1's
`docker_spawn_smoke` weak assertion, and the tautological-adjacent
`hostile_probe_runs_inside_the_same_hardened_boundary`). No `#[ignore]` anywhere
in the workspace; `0 ignored` across all 31 binaries. The env-var-gated skip in
F-1 is the only conditional test path.

### F-8. Scope note: `sharpebench run` still executes untrusted code on the host, by design

The sandbox module protects the arena path only. `sharpebench run --cmd <...>`
spawns arbitrary host commands with no gate at all
(`crates/sharpebench-cli/src/main.rs:1255, 1269, 1297`). This is disclosed
prominently in `README.md` ("`sharpebench run` executes whatever agent you point
it at **without sandboxing**; only run agents you trust"), so it is not a
finding against this commit. Recording it because the build-list phrasing
"no path where untrusted agent code can execute on the host" is true of the
*arena*, and false of the CLI as a whole — the refusal property is correctly
scoped, not global, and the disclosure is what makes that acceptable.

---

## Summary table

| # | Item | Verdict |
|---|---|---|
| 1 | Sandbox refusal-to-fall-back preserved and hardened | BUILT (live boundary unverified — F-1) |
| 2 | Protocol field contract +105 | BUILT; **not additive**, breaking change mislabelled `Fixed`; no schema exists to drift |
| 3 | `local_open_weight_field_eval.rs` | PARTIAL — real code, no stub agent, but depends on absent `sharpearena` module; 0 tests |
| 4 | Model identity in evidence | PARTIAL — digest/quant/server excellent; `max_tokens`, timeout, temperature/seed missing; identity self-reported |
| 5 | Transport integrity not weakened | BUILT — semantic faults route through the same typed flagging; Python side closed the loop |
| 6 | `engine.rs` +85 | BUILT — goldens verified byte-identical at HEAD by re-running two producers |
| 7 | CLI additions | BUILT — work as advertised; tests cover parsers only, not wiring; `sandbox-check` undocumented outside `--help` |
| 8 | Appendix A coverage | PARTIAL — local path documented and honestly labelled "(unrun)"; `sandbox-check` absent; prerequisite for the local path unstated |

## What is genuinely well done

- The refusal logic was split into a pure `resolve_launch(docker_present, ...)`
  so the security property is testable with no Docker installed. That is the
  right shape for a security boundary, and the four tests around it are real.
- The hostile probe checks the things that actually matter (`CapEff`,
  `NoNewPrivs`, netns contents, `ro` on `/`, tmpfs `noexec`) rather than
  asserting the flags were passed, and it runs inside the same argument vector
  production uses so the two cannot drift.
- The `.partial` + atomic-rename discipline in the field examples, and the
  runtime-vs-agent-fault split that decides abort-vs-score. Getting that
  distinction right is the difference between an evidence file and a rumour.
- Closing the masked-hold loop in `llm_agent.py`, *including the cached-replay
  branch*, with a comment saying why. Replay paths are where this class of bug
  comes back, and it was anticipated.
- Removing the two silent repairs in the Python parser (weight clamping and
  over-allocation rescaling). Both quietly changed results.
- Labelling the new capability "(unrun)" in the paper appendix and stating it
  supports no result. The temptation to imply otherwise was available and
  declined.
- The `has_short_target` financing gate is a hack, but it is a documented hack
  at the site of the hack, with the reason — and I verified its claim
  empirically rather than trusting it.

## Highest-value fixes, in order

1. Run `check_sandbox_readiness` in a CI job with a real Docker daemon (F-1).
   Nothing else in this commit is unverified in the same way.
2. Say out loud that the closed `Decision` contract is breaking for entrants, and
   decide the rule for which protocol types are closed (item 2).
3. Record `max_tokens`, timeout and sampling parameters (temperature/seed) in the
   local field record (item 4) — cheap, and the reproducibility claim needs it.
4. Fix `qwen3.8:27b` (F-4) and the "built and tested" CHANGELOG line (F-5).
5. Require an explicit output path in the evidence producers, and gitignore the
   stray defaults (F-2).
