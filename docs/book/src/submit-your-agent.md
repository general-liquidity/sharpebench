# Submit your agent

This page takes an agent from an empty directory to a checked submission in
about five minutes, and then explains each gate the check reports.
[Submitting an agent](submitting.md) has the full wire contract and the rules
behind every gate.

## The five-minute path

First, write a template. `init-agent` writes a minimal agent for one transport:
the policy, a README and a `sharpebench-agent.json` spec that says how to reach
the agent.

```sh
sharpebench init-agent my-agent --kind cmd
```

The `cmd` template is `agent.py`, a Python program that reads one
`MarketObservation` per line on stdin and writes one `Decision` per line on
stdout. Its policy holds an equal weight in every symbol whose last close is
above the mean of the ten closes before it. The template calls `python3`, or
`python` on Windows; pass `--python <interpreter>` to name another one.
`init-agent` never overwrites a file, so it refuses a directory that already
holds one of the files it would write.

Second, check it from the directory that holds `my-agent`:

```sh
sharpebench check-agent my-agent
```

The report has one line per gate. For the unchanged template on Linux it reads:

```text
check-agent: cmd:python3 /home/you/my-agent/agent.py
synthetic check window: 4 symbols, windows 20-50 and 50-80, seeds 0 and 1

  pass      launch       `python3` started as a host process
  pass      transport    4 runs completed with a fresh agent each
  pass      protocol     120 decisions parsed and matched the observations they answered
  pass      budget       the slowest decision took 85 ms of the 30000 ms budget
  pass      determinism  re-executing all 4 runs with fresh agents reproduced every one of 120 decisions
  advisory  verdict      headline verdict no_evidence_of_skill: pass^k failed: ...
  advisory  network      under --cmd the agent can reach the network, so its row on a historical window is published noncertifying (evidence mode backtest_unobserved). ...

no gate failed. Field it with:
  sharpebench run --cmd "python3 /home/you/my-agent/agent.py"
```

The timing and the verdict line depend on your machine and your policy.
`check-agent` exits 0 when no gate failed, 1 when at least one did, and 2 on a
usage error, so it can run in a script or a CI job. `--json` prints the same
report as one object (schema `sharpebench.check-agent.v1`) with `ok`, the
`gates` list (`gate`, `status`, `detail`), the full `headline` record and
`run_with`, the `run` flag and value that field the same entrant.

Third, replace `decide` in `agent.py` with your own policy and run the check
again until no gate fails.

Fourth, field it with the command the check printed. `sharpebench run` ranks
the agent beside the reference agents and the luck floor on the real windows.

### The HTTP and image templates

`--kind http` writes `server.py`, which serves `POST /decide`. The harness
neither starts nor stops an HTTP endpoint, so start it first and then check the
address it prints:

```sh
sharpebench init-agent web-agent --kind http
python3 web-agent/server.py --port 8765
sharpebench check-agent web-agent          # in another shell
```

`--kind image` writes the same `agent.py` plus a `Dockerfile`. The harness runs
only a digest-pinned reference (`repository@sha256:...`) that is already
present on the host, and it never pulls, so build and pin the image before you
check it. The generated README shows the commands.

```sh
sharpebench init-agent boxed-agent --kind image
docker build -t boxed-agent boxed-agent
sharpebench check-agent --image <repository@sha256:...>
```

`check-agent` also takes an entrant directly, with the same flags `run` uses:
`--cmd "<prog>"`, `--http <addr>` or `--image <repository@sha256:...>`. It
accepts `--short-borrow-bps <bps>` as `run` does. A word of a spec's `cmd` that
names a file in the agent directory is resolved against that directory, so the
check runs from any working directory.

## What the check runs

The check uses the same transports, failure taxonomy and re-execution verifier
as `sharpebench run`, `capture` and `verify-trajectory --reexecute`. It adds no
rule of its own and produces no board.

1. It starts the entrant once to see whether it can be reached.
2. It drives a fresh agent through each of four runs on a synthetic dataset of
   4 symbols and 80 bars: windows 20-50 and 50-80, execution seeds 0 and 1, 30
   decisions per run. The first fault stops the check.
3. It re-executes all four runs with fresh agents and compares every decision.
4. It ranks the entrant into a field with buy-and-hold, momentum and three
   luck-floor agents on the same window, and reads the entrant's headline
   verdict.

A `--cmd` entrant runs on the host with no sandbox, once per run, and the check
prints a warning on stderr, as `run --cmd` does.

## What each gate means

Every gate reports one of four statuses. `pass` means the gate held. `fail`
means the board would refuse the entrant here or record a failing run.
`advisory` means the finding is reported beside the score and never refuses
anything. `not_run` (printed as `not run`) means an earlier failure stopped the
check first.

### launch

The harness could not reach the entrant at all. For `--cmd` the program did not
start: it is missing, or it is not on PATH. The agent gets a cleared
environment with PATH and platform essentials only, plus the variables named in
`SHARPEBENCH_AGENT_ENV`. For `--http` nothing accepted a TCP connection at the
address within two seconds. For `--image` the sandbox refused the reference
before any container started: Docker is not reachable, the reference is not
digest-pinned, or the image is not present locally.

### transport

The exchange broke for a reason the harness cannot charge to the agent: the
agent process exited before it answered, it closed stdout, the endpoint reset
the connection, or a fresh agent could not be started for a later run. When the
process exited, the detail names the exit status; run the command by hand and
feed it one observation line to see its error output. On the board such a run
is retried, and a sweep whose retries run out is not certifying. See
[Decisions must be deterministic under re-execution](submitting.md#decisions-must-be-deterministic-under-re-execution)
for how retries work.

### protocol

The agent answered with a decision the closed wire contract refuses: output that
is not one JSON object per line, a key the schema does not name, an order for a
symbol the observation did not offer, two orders for one symbol, a
`target_weight` outside [-1, 1], or a `confidence` outside [0, 1]. The
transport prints a line beginning `agent protocol fault:` on stderr that names
the exact problem. On the board the run becomes a failing sentinel run and
counts against pass^k. See
[The wire contract is published, and it is closed](submitting.md#the-wire-contract-is-published-and-it-is-closed).

### budget

The agent took an observation and did not answer within the 30 second
per-decision budget, or, under `--image`, the container exceeded its memory
limit and the kernel stopped it. Either is the agent's own fault: the board
does not retry the run, and it counts against pass^k. When the gate passes, the
detail reports the slowest decision of the check. A host process or an HTTP
endpoint has no memory limit, so the check can test the memory budget only
under `--image`.

### determinism

Re-executing a run with a fresh agent produced a different decision. The detail
names the run, the step and the observation, and shows both decisions. Only the
score-bearing part is compared: each order's `symbol`, `action`,
`target_weight` and `confidence`, and the `cost` report. `reasoning` and
`rationale` may differ. A decision may depend only on the observations of its
run and the agent's own earlier decisions in that run, so reading the clock,
drawing ambient random numbers or keeping state in files between runs fails
here. `verify-trajectory --reexecute` refuses such a trajectory (see the
[CLI reference](cli.md#capture--verify-trajectory)).

### verdict

The entrant's [headline verdict](reading-a-row.md#the-headline-verdict) on the
check window, with each hard reason and advisory reason written out. The
[reasons table](reading-a-row.md#the-eleven-conditions-and-the-reasons-they-produce)
names the row field behind each one. Two windows of 30 synthetic bars are far
too short to show skill, so most agents read `no_evidence_of_skill` or
`cannot_tell` here. The gate is therefore advisory and passes only on
`skill_supported`. Its use is to show the reasons the scorer gives and to catch
a track the scorer cannot read at all, such as a `degenerate_track` from an
agent that never trades.

### network

Whether a row from this transport can certify on a historical window. Only
`--image` runs the entrant with `--network none`, so only an image row can
certify there. Under `--cmd` or `--http` the agent can reach the network, and
the row is published with the evidence mode `backtest_unobserved` and marked
noncertifying. Score and rank do not change. The gate needs no run, so it is
reported even when the launch fails. See
[Network reach and evidence mode](submitting.md#network-reach-and-evidence-mode)
and [Three meanings of "certifying"](reading-a-row.md#three-meanings-of-certifying).

## What the check does not cover

The check runs on synthetic bars and makes no claim about how the agent scores
on real data. It does not arm a fault plan, attach a model gateway, run the
image preflight (`--scan-policy`), or look for state carried between cells:
the `cell_isolation` disclosure on a board row says what each runner allows,
and nothing detects carryover. An HTTP endpoint is checked as it runs now, and
the check cannot see whether it keeps state between runs in memory.
