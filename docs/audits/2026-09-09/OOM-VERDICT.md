# The sandbox's out-of-memory verdict

The CI job "live container boundary (hostile probe)" failed intermittently in
`sandbox::tests::live_memory_limit_sets_the_oom_killed_verdict`
(`crates/sharpebench-arena/src/sandbox.rs`). This note records what the
failures show, what the Docker, containerd and kernel sources establish about
the cause, what stays unproven, and the classification rule that replaced the
single-flag read in production.

## The failure

The test runs the hardened inspectable launch with a 32 MiB `--memory` and
`--memory-swap`, `exec`s `dd` as namespace PID 1 and writes 64 MiB into the
`/tmp` tmpfs, whose pages are charged to the container's memory cgroup. It then
read `docker inspect --format {{.State.OOMKilled}}` through `DockerCli`, the same
read `SandboxedAgent::finish` made in production.

Four failed attempts, all on `ubuntu-latest` with Docker 28.0.4, containerd
v2.3.4, runc 1.5.1 and the systemd cgroup v2 driver:

| Job | Run (attempt) | Branch |
|---|---|---|
| 102848350719 | 34470289646 (1) | main |
| 102912365979 | 34489540517 (1) | feat/contract-ports |
| 102919159065 | 34491514208 (1) | feat/contract-ports |
| 102954394430 | 34501878363 (1, attempt 2 passed) | fix/gateway-docs-evidence-verdict |

Every one has the same signature: the first assertion (`docker run` exited
non-zero) passed, and the flag read `Ok(false)` where `Ok(true)` was expected.
In the same four runs `live_surviving_wrapper_child_oom_is_recorded`, whose
container survives the child's OOM kill and exits 0 later, printed
`State.OOMKilled=Ok(true)`. Over the 60 most recent `ci.yml` runs the job's
latest attempt failed 3 times and succeeded 55 times (2 cancelled), and the
fourth failure above was cleared by a rerun, so the rate is roughly one attempt
in fifteen.

The logs do not record the container's exit code: the old test asserted only
`!status.success()`. The rewritten test prints the `docker run` exit code, the
full inspected state and the verdict on every run, and asserts exit 137.

## What the sources establish

**Docker sets `State.OOMKilled` only from an event, independently of the exit.**
In moby v28.0.4, `daemon/monitor.go` `ProcessEvent` sets `c.OOMKilled = true`
on `EventOOM`, and `handleContainerExit` records the exit code and calls
`SetStopped`. `container/state.go` `SetStopped` does not touch `OOMKilled`;
only `setRunning` clears it. `libcontainerd/remote/client.go` maps containerd's
`TaskOOM` to `EventOOM` and runs events through a per-container FIFO
(`c.eventQ.Append`), so the daemon applies the flag and the exit in the order
containerd delivers them. `docker run` returns once `SetStopped` notifies its
waiters, so an OOM event delivered after the exit is not visible to an inspect
issued right after `docker run` returns, and one never delivered is never
visible.

**The OOM event is produced asynchronously and can be lost.** In containerd
v2.3.4 the runc shim watches `memory.events` through inotify
(`internal/oom/watcher.go`) and publishes `TaskOOM` when `oom_kill` rises.
Commit 8ac7e3c06d ("use experimental OOM package", PR #12714) makes the shim
stop the watcher, which performs a final `memory.events` read, before it sends
`TaskExit` (`cmd/containerd-shim-runc-v2/task/service.go`,
`handleProcessExit`). Its message states the intent: "We should always send
oom event before exit event." Two paths in the same code still lose or delay
the event:

- If the final read finds `memory.events` gone, the watcher returns without
  publishing (`readKVStatsFile` fails and `os.Lstat` reports not-exist).
- `RemoteEventsPublisher.Publish` (`pkg/shim/publisher.go`) forwards
  synchronously, and on a failed forward requeues the event after a delay of
  at least one second, so it can reach the daemon after `TaskExit`.

The race is acknowledged upstream. Commit 842cb99a5e ("monitor OOM event after
creation") says: "There is a race condition where the exit-event goroutine may
clean up the task and update its status faster than the OOM event updater. I
don't have a better idea to fully resolve this race condition, but this patch
aims to minimize the chance of missing OOM events." containerd issue #8893
("TaskOOM event lost", open) documents a kernel-confirmed memcg OOM kill with
exit 137 and no `TaskOOM`, a reproduction with a memory limit and a tmpfs write
(the shape of this fixture), and a report that it still happens on containerd
2.1.4. Issue #8180 is the Kubernetes-side symptom (`Error` instead of
`OOMKilled` for a container OOM-killed right after start).

**The exit code is recorded synchronously with the exit.** The shim's reaper
reports a signalled process as `128 + signal`
(`pkg/sys/reaper/reaper_unix.go`, `exitSignalOffset = 128`), carried in
`TaskExit.ExitStatus` and stored by `handleContainerExit` as `State.ExitCode`.
A SIGKILL is 137 in the same state write that marks the container exited.

**The kernel counts the kill before it sends it.** `__oom_kill_process`
(`mm/oom_kill.c`, v6.8) calls `memcg_memory_event_mm(mm, MEMCG_OOM_KILL)`
before `do_send_sig_info(SIGKILL, ...)`, under the comment "Raise event before
sending signal: task reaper must see this". The cgroup v2 documentation
defines `oom_kill` as "The number of processes belonging to this cgroup killed
by any kind of OOM killer". `memory.events` is therefore reliable while the
cgroup exists, but it is host sysfs: the harness talks only to the `docker`
client, and by the time `docker run` returns the daemon has deleted the task,
so it is not an input the finalizer can read after the exit.

**Who can SIGKILL the entrant.** The inspectable launch omits `--init`, so the
image entrypoint is namespace PID 1. `pid_namespaces(7)`: "Only signals for
which the "init" process has established a signal handler can be sent to the
"init" process by other members of the PID namespace", and SIGKILL "is
forcibly delivered when sent from an ancestor PID namespace". The entrant and
its children therefore cannot produce a SIGKILL death of PID 1. On the harness
side, the finalizer inspects before `docker rm -f`; a decide timeout
(`crates/sharpebench-sim/src/external.rs`) SIGKILLs the `docker` client's own
process group, which the container's processes (children of the containerd
shim) are not in; and the teardown's SIGTERM reaches the container at most as
a proxied SIGTERM (`docker run --sig-proxy`, default on without a TTY), which
cannot be SIGKILL. `--pids-limit`, the tmpfs size, `nofile` and the default
seccomp profile refuse rather than kill.

## What is not established

- Which loss path produced these four failures. The job collects neither the
  containerd log (a failed forward logs "forward event") nor the kernel log,
  and the flag was read once, so a lost event and a late one are
  indistinguishable in the evidence.
- That the four failures exited 137. It is the expected exit of an `exec`'d
  writer crossing a no-swap 32 MiB limit, but a write that fails with an error
  instead of a kill is not ruled out by anything recorded. The rewritten test
  asserts 137, so a different exit now fails on its own message.
- Whether the systemd driver prunes the container's cgroup before the shim's
  final read. It would explain the first loss path under this CI
  configuration; nothing here observed it.
- The local Docker daemon was not available (`docker version` did not answer
  within 15 s), so no local reproduction was attempted.

## The classification rule

`classify_container_exit` decides from one `docker inspect` of status,
`State.OOMKilled` and `State.ExitCode`, taken before the harness signals the
container:

| Status | OOMKilled | Exit code | Verdict |
|---|---|---|---|
| any | true | any | `OomKilled(Recorded)` |
| `exited` | false | 137 | `OomKilled(UnrecordedSigkill)` |
| `exited` | false | any other | `WithinBudget` |
| `running` | false | (none) | `WithinBudget` |
| `paused`, `restarting`, `removing`, `dead`, `created` | false | any | indeterminate error |

`SandboxedAgent::finish_with` returns the typed `ResourceVerdict`;
`SandboxedAgent::finish` keeps its `Result<Option<bool>, SandboxError>`
contract for the CLI (`Some(true)` for either breach, an indeterminate state
as `SandboxError::Inspection`). Before classifying, the finalizer re-reads a
container still reported `running` without an OOM record for up to 3 seconds,
because the `docker` client can be reaped before the daemon records the exit.
A container still running after that is an alive entrant, typically one that
timed out and ignored EOF, and is within budget. The teardown removal comes
after the read, so its own SIGKILL is never classified.

A timeout cannot become a breach through this rule: the harness never SIGKILLs
the container before the read, a timed-out entrant still running is within
budget, and one that exits after the timeout reports its own exit code.

## Residual limits

- **Out-of-band kills.** A host actor with Docker or root access (`docker
  kill`, `docker stop` past its grace period, a daemon shutdown) can SIGKILL the
  entrant mid-run, and the result is classified as a budget breach. That is
  outside the benchmark's operating contract, and Docker records nothing that
  would distinguish it.
- **A voluntary `exit(137)`** is classified as a breach. It makes the run a
  non-retried agent fault, so it can only cost the entrant.
- **A surviving wrapper.** An entrypoint that forks the real agent, survives
  the child's OOM kill and exits 0 gives no exit-code evidence, so the verdict
  rests on `State.OOMKilled` alone and inherits the event race. The surviving
  wrapper probe observed the flag set in every recorded run, where the event
  has the container's remaining lifetime to arrive.
- **A global OOM kill** also exits 137 and also sets `oom_kill`; neither
  Docker nor this rule separates it from the cgroup budget.
- **A settle window overrun.** A daemon that takes longer than 3 seconds to
  record an exit leaves the container `running` at the last read, which is
  classified within budget unless the OOM record already landed.

## Tests and mutation checks

Unit tests with the injected `ContainerInspector` (no daemon needed):
`the_resource_verdict_covers_every_status_flag_and_exit_code`,
`the_docker_inspect_state_line_parses_strictly`,
`finish_inspects_before_removing_and_reports_the_verdict`,
`a_harness_teardown_kill_is_never_classified_as_a_breach`,
`a_running_container_is_re_read_until_its_exit_is_recorded`,
`finish_refuses_an_indeterminate_verdict_or_failed_cleanup`,
`an_unsandboxed_run_has_no_container_and_no_verdict`. The live test
`live_memory_limit_sets_the_oom_killed_verdict` asserts exit 137 and a breach
through the production `settled_exit_state` and `classify_container_exit`.

Each mutant was applied to `sandbox.rs` in the isolated worktree, the arena
sandbox tests run, and the file restored from `git show HEAD:` and confirmed
with `cmp`:

| Mutant | Killed by |
|---|---|
| drop the exit-137 rule | verdict table, finish, settle re-read |
| ignore `State.OOMKilled` | verdict table, finish, settle re-read |
| remove before inspect | teardown kill, finish, settle re-read, indeterminate |
| no settle re-read | settle re-read |
| unknown status treated as within budget | verdict table, indeterminate |
| 143 (SIGTERM) instead of 137 | verdict table, finish, settle re-read |
| `running` treated as indeterminate | verdict table, teardown kill |

## Sources

- moby v28.0.4: `daemon/monitor.go`, `container/state.go`,
  `libcontainerd/remote/client.go`
- containerd v2.3.4: `internal/oom/watcher.go`,
  `cmd/containerd-shim-runc-v2/task/service.go`, `pkg/shim/publisher.go`,
  `pkg/sys/reaper/reaper_unix.go`; commits 8ac7e3c06d and 842cb99a5e
- containerd/cgroups v3.1.3 `cgroup2/manager.go` (`EventChan`, the older
  watcher with the same not-exist return)
- [containerd #8893, TaskOOM event lost](https://github.com/containerd/containerd/issues/8893)
- [containerd #8180, reason not OOMKilled for containers killed right after start](https://github.com/containerd/containerd/issues/8180)
- Linux v6.8 `mm/oom_kill.c` `__oom_kill_process`
- [Linux cgroup v2 documentation, memory.events](https://docs.kernel.org/admin-guide/cgroup-v2.html)
- [pid_namespaces(7)](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html)
