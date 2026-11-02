# Running a live window

This is the operator runbook for `arena live-window`, the one command that runs
a forward window end to end with every entrant under
[live forward capture](arena.md#live-forward-capture). It opens the window,
drives each committed image bar by bar through the sandbox, writes the
append-only live logs, checks the host clock, seals the logs at close, scores,
publishes and verifies. Running it again on the same directory resumes after a
restart.

```text
sharpebench arena live-window <dir> \
  --window <spec.json> \
  --entrants <manifest.json> \
  --data-feed replay:<dataset.csv> \
  --key env:ARENA_KEY \
  [--simulated-clock] [--json]
```

The exit code is 0 for a published board that verified, 1 for any failure and
2 for a usage error.

## Before the window

**The window spec** fixes everything the logs depend on, before the window
opens:

```json
{
  "schema": "sharpebench-arena-live-window/v1",
  "window_id": "live-2026-12",
  "commit_deadline": 10,
  "data_reveal_epoch": 20,
  "bars": 60,
  "scorer_artifact_sha256": "<64 hex>",
  "score_config": { "execution_seeds_per_window": 2 }
}
```

`bars` counts every bar the window receives, warm-up history included. The
execution matrix is the one every capture runs: market windows `[w, m)` and
`[m, bars)` with `w = clamp(bars / 10, 10, 30)` and `m = (w + bars) / 2`, and
the config's execution seeds. Every log header names that matrix, so the bar
count cannot change once the window opens, and it must be at least 40.
`score_config` is optional; the default config applies without it. The epochs
are the arena's integer epochs (see [the arena](arena.md)).

**The entrants manifest** lists the field:

```json
{
  "schema": "sharpebench-arena-live-entrants/v1",
  "entrants": [
    { "agent_id": "desk-a", "image": "registry.example/desk-a@sha256:<64 hex>", "salt": "<secret salt>" }
  ]
}
```

Each image must be digest-pinned and launchable by the sandbox. The command
registers each entrant's commitment (agent id, window, image digest, salt), or
checks that one an entrant registered itself with `arena commit` is that
commitment. The salt is revealed at close, as for any reveal.

**The key** is the arena signing key, in the `env:NAME` / `file:PATH` /
literal convention. It seals the logs and signs the board; publishing refuses
a log sealed under any other key.

Before anything is written the command checks the spec and the manifest,
confirms Docker is running, and has the sandbox admit every image (it must be
present locally). A failure here leaves the directory untouched.

## During the window

The command creates the arena if `<dir>` holds none, opens the window with the
spec's scorer and config, registers the commitments and advances to the commit
deadline, so the field is closed before the first bar. It then takes bars in
order. For each bar it:

1. appends the bar to `live/<window>/feed.jsonl` and syncs it to disk;
2. checks the host clock (below);
3. for every run of the execution matrix the bar belongs to, starts the run's
   instance if it has none (a fresh hardened container, network disabled),
   hands it the point-in-time observation the simulator builds from the bars
   so far, and appends the observation digest and the decision to that
   entrant's live log in `live/<window>/`, syncing each entry.

An instance lives for its run and is stopped after the run's last bar, so at
most one container per entrant and seed runs at a time.

**A stalled entrant** is charged under the transport's own budget. Each
decision has 30 seconds, net of the time the host spends serving the entrant's
model requests. An entrant that overruns it is stopped and holds for every
later bar of that run; an invalid decision or an early exit holds that bar,
and three faults in a row open the circuit breaker, after which every bar
holds. The holds are the decisions applied, so they are what its log records,
each marked `host_hold`, and the report lists each run's first fault
(`run 0: BudgetOverrun`). The count of those holds is sealed into the entrant's
live capture root as `stalled_holds` and published on the signed board:
`board.md` lists it beside the row's provenance (`live-captured,
stalled_holds 12`), the [leaderboard export](leaderboard-export.md) shows it as
the row's `stalled_holds` qualifier, and the run report gives it per entrant.
A log that marks a decision with orders as a host hold is refused at intake.
Model calls it made before it stalled were
dispatched, so the gateway journal charges them by its
[settlement rules](model-gateway.md#reservations-and-settlement). A container
that cannot start holds the whole run the same way.

**Clock checks** apply the owner-confirmed conventions of live capture: UTC
bar days and a two-minute tolerance. Before a bar is handed to any entrant the
host clock must have reached the start of that bar's day, and when a bar
arrives, every decision on the previous bar must have been recorded before the
new bar's day began. Either failure stops the run, because every log of the
window would already fail intake and further bars would only spend model
calls. The last bar's deadline is checked by intake at close.

## Restarts

Run the same command again. It resumes from the record:

- a feed line cut short by a crash is dropped; no entrant was shown that bar,
  because a bar is handed out only after its line is on disk;
- each log is read and checked against the feed journal (every run must have
  decided every stored bar, or every one but the last) before a restart entry
  is appended, so a resume that is refused writes nothing;
- the bar that was open when the host stopped is shown to a fresh instance
  again before it is decided, as live capture allows after a restart entry;
- a changed spec or manifest is refused: copies of both are kept in
  `live/<window>/` and must match.

Each restart starts the affected entrants as fresh instances in the middle of
the window, and the board publishes the restart count beside the row. A host
that stays down past a bar's deadline cannot recover that window: the clock
check stops the resumed run.

## At close

After the last bar the command seals each log with the arena key, writes the
revealed dataset (`live/<window>/dataset.csv`, the bars exactly as the
entrants were shown them) and the entries (`live/<window>/entries.json`, each
carrying its sealed log), advances to the data-reveal epoch, scores, publishes
and verifies under the key's public half. A log that fails intake is refused
and recorded on the board with its reason. Running the command on a scored
window only publishes it, and on a published window only verifies it.

The report (`--json` gives it whole) names, per entrant, its log, the faults
seen by this invocation, and the bar, model call and restart counts of its
admitted log, then the ranked count, the refusals, each row's provenance,
whether the board certifies, the board path and the verifying key.

## Data feeds and rehearsals

`--data-feed replay:<dataset.csv>` serves the bars of a `date,symbol,close`
file. No live market-data adapter ships: one needs market-data infrastructure
and data rights. An operator binary supplies one by implementing `LiveFeed`,
whose `bar(index)` blocks until that bar exists, and calling
`sharpebench_arena::run_live_window` itself.

A replay of past bars under the wall clock stops at the first clock check,
since its decisions are years late. `--simulated-clock` rehearses a window on
a replay: the clock moves to 21:00 UTC on each bar's day as the bar arrives.
Its log timestamps are bar times, not the host's, so every log header records
`simulated_clock`, every row is `live-rehearsal` rather than `live-captured`,
and the board never certifies: `board.md` opens with the noncertifying notice
and the evaluation tier `historical-advisory`. Verification refuses a re-signed
header that claims certification for such a board, and refuses a rehearsal row
relabelled `live-captured`, because each root records the simulated clock. A
window opened under the simulated clock cannot be resumed under the wall clock.
A rehearsal belongs in its own arena directory under its own key; the report
carries `simulated_clock: true` and the JSON output also marks it with
`"clock": "simulated"`.

## Model-backed entrants

The CLI runs images under the sandbox with no model gateway attached, because
no provider adapter ships (see [the model gateway](model-gateway.md#what-an-operator-must-supply)).
A model-backed entrant runs from an operator binary that implements
`LiveHost`: its `launch` starts the instance through `gateway_launch`, serves
it with a `GatewayEntrant`, and wraps its provider transport in
`LoggingTransport` over the recorder it is handed, so every dispatched call is
logged beside the bar it was made for.
