# Rust trace validation and schedule fuzzing

These local tools connect the [lifecycle specifications](../../specs/README.md)
to observed Rust supervisor decisions. They are opt-in, require no Python
packages, and do not install tools or download dependencies themselves.
Generated traces and checker output belong outside the source tree; no historical
reports or campaign archives are required.

## Prerequisites

- Rust/Cargo, Python 3 and Java.
- The official TLC 1.8.0 jar described in `specs/README.md`. Set `TLC_JAR` to its
  absolute path, or put it at the repository root as `tla2tools.jar`. The tools
  enforce SHA-256
  `011267fca25d0752cf18894dd585cd28b96960c112ea8a20b823ea770e7e10cd`;
  there is no fallback to another checker.
- Linux for real-process campaigns (`/proc` identity checks and child subreaping).

Run commands from the repository root. Normal Horust builds do not enable
tracing; `formal-trace` enables the optional JSON dependency and capture hooks.
No protobuf compiler is needed for normal builds; development protobuf generation
is documented separately in `commands/README.md`.

## Local checks

```bash
# Check all standalone models, including refinement and expected cycle failures.
python3 tools/formal/check.py models
python3 tools/formal/check.py models --only SupervisorTwo HorustIndependentFailure

# Build the instrumented binary and run Python validator/runner/cleanup tests.
python3 tools/formal/check.py test

# Execute seeded production-handler schedules with virtual time/process effects.
python3 tools/formal/check.py fuzz schedule --count 32

# Execute real Horust subprocesses with isolated services and bounded leaf helpers.
python3 tools/formal/check.py fuzz process --count 8
```

Outputs default to fresh directories under the system temporary directory.
`models`, `validate`, `minimize`, `fuzz` and `replay` accept `--out` for a new path;
an existing directory is never overwritten. The `test` command has no `--out`:
individual tests retain their own temporary artifacts. Rust tests require the
normal user environment (including `USER`) for sample-service expansion.

The model runner excludes the generated-data `TraceValidation` scenario from its
standalone matrix. Both startup and shutdown cycles are expected negative cases;
an incorrect exit code or wrong violated property is a failed check.

Campaign manifests retain seeds, source fingerprints, results and failure reasons.
Cases are **accepted**, **rejected** (trace/schema/causality or model violation), or
**inconclusive** (tooling failure, watchdog or uncertifiable observation).
A campaign exits zero only if every case is accepted. Inspect a rejected case's
`check/result.json` and `check/tlc.log` rather than treating all nonzero runs as
the same problem.

## Capture an actual execution

```bash
cargo build -p horust --features formal-trace
HORUST_FORMAL_TRACE=/tmp/new-horust-trace.ndjson target/debug/horust -- /bin/true
python3 tools/formal/check.py validate /tmp/new-horust-trace.ndjson
```

The trace filename must not exist. Traces are versioned NDJSON with an initial
snapshot and completion footer. Service names are aliased; commands and
environment values are not serialized into the trace. Campaign logs/config files
can contain commands and paths, so review artifacts before sharing them.

Capture records owner mutations, preparation handoffs, births, reaps, signals,
deadlines, worker attempt identities and bus dispatch/receipt order. Missing or
unexplained steps, wrong identities, incomplete accounting and invalid transitions
are rejected. Capture I/O errors are fatal rather than silently dropping evidence.
Tracing adds locks and flushes and can perturb timing; it is a debugging facility,
not a production telemetry mode.

Each iteration also records its ordered publication batch. The validator rebuilds
that batch independently from owner-step outputs, accounted exits and generated
commands, then matches every owner send in order. The publication record is
mandatory: older research traces without it are not accepted by this importer.

The generic public Rust `BusConnector::send_event` does not provide typed trace
payloads. For executions injecting events through that API, capture may be
uncertifiable; a null payload must not be interpreted as a valid hidden step.

## Replay and reduce a counterexample

```bash
python3 tools/formal/check.py replay schedule 19
python3 tools/formal/check.py replay process 2
python3 tools/formal/check.py minimize /tmp/failing-case/trace.ndjson
```

Deterministic schedules retain legal bus ordering while varying worker delivery,
health and process outcomes. They exercise production preparation, event handlers,
command generation and exit accounting; only time and OS fork/signal effects are
virtualized. Real-process replay regenerates a seeded scenario/signal plan, not
an identical kernel interleaving.

The validator projects data/provenance; `specs/TraceValidation.tla` checks the
transition and output relations. Generated bundles include their source NDJSON
and TLA inputs. Attempt IDs retain their raw u64 values in NDJSON; finite rank
encoding preserves identity equality, observed successors and exhaustion in TLC.

`minimize` applies only to a mapped TLC `Accepted` invariant failure. It retains
the complete original input, shortens the projected Actions sequence through the
first failing action, and reruns TLC to reproduce the same violation.
This certifies a shorter **action prefix**, not a globally minimal causal trace.
Initial-state, provenance, incomplete, tooling and accepted traces are not
reported as successfully minimized.

## Limits

The standalone Horust-to-Supervisor refinement remains a model-to-model result.
Finite Rust traces provide observed conformance, not general liveness, arbitrary
Rust refinement or proof that no bugs remain. The model abstractions, finite
queue/scenario bounds and environmental fairness assumptions still apply.

Real helpers are time-bounded and cleanup targets recorded child PID/start-time
identities rather than process names or groups. Interrupted/watchdog runs remain
evidence of an incomplete run, never successful shutdown. These campaigns should
run on a development machine, not against production services.
