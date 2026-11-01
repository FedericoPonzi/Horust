# Lifecycle specifications

These TLA+ models describe Horust's service lifecycle and coordination contracts.
They check bounded safety and conditional progress; they are not a proof that
arbitrary Rust executions refine the models.

| Module | Scope | Configurations |
| --- | --- | --- |
| `Supervisor.tla` | Abstract single-shot supervisor contract: admission, ownership, readiness, direct failure effects and retirement | `SupervisorMC` / `SupervisorTwo` |
| `Process.tla` | One service: spawn, registration, readiness, exit/reap and shutdown | `ProcessStartup`, `ProcessFailures`, `ProcessIgnoreTerm` |
| `ProcessExtended.tla` | One service with health, restart budgets, delays and termination | `ExtendedStartup`, `ExtendedFailures`, `ExtendedTerm` |
| `ProcessRules.tla` | Shared pure state-transition and policy functions | Used by `ProcessExtended` |
| `Horust.tla` | Two/three services, bounded command delivery, dependencies and failure propagation; refines `Supervisor` | `Horust*.cfg` |

## Checked refinement

Every Horust configuration checks `PROPERTY AbstractSpec`, the instantiated full
`Supervisor!Spec`, including its fairness clauses, not just abstract invariants.
The concrete `Spec` therefore implies this abstract specification for each checked
finite configuration (the two negative configurations separately fail progress).
The explicit mapping is:

- `phase` is unchanged; Initial maps to Waiting, Starting/Started to Preparing,
  Running to Up, InKilling to Stopping, and outcomes to ExitOk/ExitFail/Done/DoneFail.
- `owned` contains services with an OS child or an exit notification still owed
  (`os # "None"`), including already-reaped children awaiting delivery.
- `active` contains unowned Starting services: an admitted attempt still awaiting
  spawn or cancellation notification. `blocked` maps the direct-victim ledger.
- Queue generation, PID registration, signal response and reaping before delivery
  stutter abstractly. Spawn changes ownership; exit delivery relinquishes it.
  Publishing spawn cancellation stutters until its failure notification arrives.

Abstract weak fairness requires admission, spawn/readiness when continuously
eligible, required cancellation, stopping, stopped-child exit and retirement.
Horust's strong fairness for per-service generation/spawn/reaping plus weakly fair
FIFO delivery and atomic timeout response discharge these obligations. Neither
level assumes fair spontaneous exit or fair global escalation. Unsolicited shutdown
is allowed only when `ExternalShutdown` is enabled, and is not required by the
abstract contract. Abstract startup success is not promised: spawning may fail.

`Process` and `ProcessExtended` remain independent checks, not refinement layers.
In particular, extended retries cannot refine the minimal single-shot process.

## Running TLC

Scenario configurations live in `configs/`; modules remain in this directory.

Use Java and the official [TLC 1.8.0 release](https://github.com/tlaplus/tlaplus/releases/tag/v1.8.0).
The jar is not committed. The checked build is `2026.10.30.154139`, with SHA-256:

```text
011267fca25d0752cf18894dd585cd28b96960c112ea8a20b823ea770e7e10cd
```

From this directory, set `TLC_JAR` to the absolute path of your downloaded jar:

```bash
export TLC_JAR=/absolute/path/to/tla2tools.jar
sha256sum "$TLC_JAR"
states=$(mktemp -d)
java -Xmx2g -cp "$TLC_JAR" tlc2.TLC -noGenerateSpecTE -workers 1 \
  -config configs/ProcessStartup.cfg -metadir "$states" Process.tla
```

Choose the configuration's module from the table above and use a fresh state
directory for each run. For example:

```bash
states=$(mktemp -d)
java -Xmx2g -cp "$TLC_JAR" tlc2.TLC -noGenerateSpecTE -workers 1 \
  -config configs/HorustFanIn.cfg -metadir "$states" Horust.tla
```

All 22 configurations were checked: 20 should finish without errors. Two are
intentionally negative (exit 13):
`HorustCycle` has cyclic startup prerequisites (`StartupProgress` fails);
`HorustShutdownCycle` starts independent services but has cyclic shutdown ordering
(`ShutdownProgress` fails). Global escalation is possible, not assumed fair, so
the second counterexample legitimately remains in graceful shutdown.

## Contracts and assumptions

The models distinguish starting a process, registering ownership, becoming ready,
requesting termination, observing exit and reaping. Sending SIGKILL does not itself
complete a service. Dependencies permit startup after Running or Finished; shutdown
dependencies require terminal states. Shutdown never returns to Running.
`NoLateSpawn` and `DependencySafety` forbid spawn/admission after shutdown or a
direct victim designation. `ReadySafety` also rejects stale victim readiness and
readiness after reaping, but allows an unreaped exited child: readiness delivery
does not establish that the kernel process is still alive.

Progress assumes fair relevant actions, progressing finite timers, eventual
spawn completion/cancellation and eventual signal response/reaping. Startup also
requires successful spawning and sustained health. A cyclic graph, permanently
failed prerequisite or unfair environment cannot guarantee startup.

Spontaneous exits are not assumed fair: shutdown progress must come from eventual
signal response rather than an assumption that services voluntarily exit.
Failure-policy configurations disable unrelated external shutdown where necessary
to avoid vacuous failure-induced progress. `HorustIndependentFailure` exercises
queued admission during failure-induced shutdown without a failed prerequisite
masking the phase guard. `FailurePreserved` checks that extended shutdown
finalization neither restarts a Failed service nor masks failure as Finished.
`ProcessIgnoreTerm` requires the fair deadline/SIGKILL path; the other minimal
process configurations permit TERM response.

## Bounds and abstractions

- The extended model explores selected policies: Always/budget 1 for startup,
  OnFailure/budget 1 for failures, and Never/budget 0 for the TERM variant.
  It does not enumerate every budget or unbounded retry history.
- The multi-service composition uses no restarts and abstract immediate readiness.
  Health/restart detail is checked separately, not as an exhaustive combined model.
- Command queues are bounded to one or two entries. Graphs are selected chains,
  fan-out, fan-in and a negative cycle. These are not cutoff proofs for arbitrary
  graph sizes or channel backlogs.
- Startup and shutdown graphs are independent configuration parameters.
  `DieIfFailed` uses the selected startup graph as its termination graph.
- Horust abstracts finite per-process kill timeout, escalation and exit response
  into the fair `TimeoutExit` action. It has no redundant TERM-response parameter;
  distinct TERM/deadline/KILL paths are checked in the two process models.
  Neither composition nor abstraction measures wall-clock latency.
- Failure victims are **direct dependents**, not the transitive closure.
  An Initial victim completes successfully, so it does not itself trigger failure
  propagation. `HorustDependentsChain` explores three services: its third service
  can run after the unstarted second service is cancelled successfully; a second
  service that instead fails can propagate failure onward. A Starting victim with
  pending preparation aborts without spawning; an already-owned child is stopped
  after registration. This contract does not claim a transitive kill policy.
- Only Ignore's blocked-prerequisite scenario and the negative startup cycle
  disable deadlock checking. Dependents/DieIfFailed scenarios retain it.

The models express intended admission and shutdown ordering, including rechecking
queued decisions at delivery. Do not infer that every implementation boundary,
kernel/PID-1 behavior, resource limit, health-worker incarnation or IPC operation
has been verified by these standalone models. Rust trace validation and fuzzing
are separate tooling, not included in this specs-only change.
In particular, the persistent victim guard and dependency recheck are desired
delivery contracts, not evidence that master implements that ledger or every
recheck; the checked relation is between TLA+ models, not from Rust to TLA+.
Dependency eligibility is checked at admission, not again at spawn; a prerequisite
can exit between those steps. Do not interpret this as a continuous dependency
availability guarantee.
