"""Project instrumented NDJSON into TLA+ data; TLC owns lifecycle decisions."""

import argparse
from collections import Counter, deque
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
JAR_SHA256 = "011267fca25d0752cf18894dd585cd28b96960c112ea8a20b823ea770e7e10cd"
EVENTS = {"Run", "Kill", "ForceKill", "SpawnFailed", "PidChanged", "StatusUpdate",
          "StatusChanged", "ServiceExited", "HealthCheck", "StartHealthCheck", "Shutdown"}
LOCAL_EVENTS = {"SpawnRegistered", "ReapedExit"}
MAX_ATTEMPT = 2**64 - 1
OBSERVATIONS = {"signals", "signal", "signal_all", "health_result",
                "worker_stop_request", "workers_stop_request", "wait_signal", "wait",
                "simulation", "schedule", "watchdog"}


# Checker/tooling failures, unmapped TLC outcomes and traces the instrumentation
# itself marks uncertifiable say nothing about lifecycle conformance.
INCONCLUSIVE = {"tooling", "tlc", "uncertifiable"}


class TraceError(Exception):
    def __init__(self, message, record=None, kind=None, stage=None):
        super().__init__(message)
        self.record, self.kind, self.stage = record, kind, stage
        self.result = None


def require(condition, message):
    if not condition:
        raise TraceError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def checker():
    jar = Path(os.environ.get("TLC_JAR", ROOT / "tla2tools.jar")).resolve()
    require(jar.is_file(), f"missing pinned TLC 1.8.0 jar: {jar}")
    require(hashlib.sha256(jar.read_bytes()).hexdigest() == JAR_SHA256,
            "TLC jar checksum differs from the approved 1.8.0 build; no fallback permitted")
    return jar


def event(value, local=False):
    require(isinstance(value, dict) and set(value) ==
            {"kind", "svc", "target", "pid", "code", "good", "mode", "attempt_id"},
            "invalid event schema")
    require(value["kind"] in EVENTS | (LOCAL_EVENTS if local else set()), "unknown event kind")
    require(all(isinstance(value[k], str) for k in ("kind", "svc", "target", "mode")),
            "event strings required")
    require(type(value["pid"]) is int and type(value["code"]) is int
            and type(value["good"]) is bool, "invalid event value types")
    require(type(value["attempt_id"]) is int and 0 <= value["attempt_id"] <= MAX_ATTEMPT,
            "invalid event attempt ID")
    require(value["kind"] in {"HealthCheck", "StartHealthCheck"} or value["attempt_id"] == 0,
            "unexpected attempt tag on event")
    return value


def snapshot(value):
    require(isinstance(value, dict) and set(value) == {"phase", "services", "pid_map"},
            "invalid snapshot schema")
    require(isinstance(value["services"], dict) and isinstance(value["pid_map"], list),
            "invalid snapshot containers")
    for process in value["services"].values():
        require(set(process) == {"status", "pid", "retries", "hc", "timer", "attempt_id"},
                "invalid process snapshot schema")
        require(isinstance(process["status"], str) and type(process["timer"]) is bool
                and all(type(process[k]) is int for k in ("pid", "retries", "hc", "attempt_id")),
                "invalid process snapshot types")
        require(0 <= process["attempt_id"] <= MAX_ATTEMPT, "invalid process attempt ID")
    pairs = value["pid_map"]
    require(all(isinstance(p, list) and len(p) == 2 and type(p[0]) is int
                and isinstance(p[1], str) for p in pairs), "invalid PID map schema")
    require(len({tuple(p) for p in pairs}) == len(pairs), "duplicate PID map pair")
    return dict(value, pid_map=frozenset(tuple(p) for p in pairs))


def configuration(value):
    require(isinstance(value, dict) and value, "empty/malformed configuration")
    keys = {"strategy", "attempts", "max_failed", "start_after", "shutdown_after",
            "failure", "die_if_failed", "successful_codes", "wait_secs",
            "healthy_after_ms", "checks"}
    for config in value.values():
        require(isinstance(config, dict) and set(config) == keys, "invalid policy schema")
        require(config["strategy"] in {"Never", "Always", "OnFailure"}
                and config["failure"] in {"Ignore", "Shutdown", "KillDependents"},
                "invalid policy enum")
        require(all(type(config[k]) is int for k in
                    ("attempts", "max_failed", "wait_secs", "healthy_after_ms"))
                and type(config["checks"]) is bool, "invalid policy types")
        for key in ("start_after", "shutdown_after", "die_if_failed"):
            require(isinstance(config[key], list)
                    and all(n in value for n in config[key]), "invalid dependency reference")
        require(isinstance(config["successful_codes"], list)
                and all(type(code) is int for code in config["successful_codes"]),
                "invalid successful exit codes")
    return value


def project(records):
    require(records and records[0]["kind"] == "header", "missing trace header")
    require(records[-1]["kind"] == "complete", "missing completion footer (incomplete run)")
    require(records[0]["data"] == {"format": "horust-formal"}, "unknown trace format")
    config = initial = owner = None
    actions, sends, queues, joins, dispatched = [], {}, {}, set(), set()
    consumed, handled = set(), set()
    pending, children, workers, runs = {}, {}, {}, {}
    spawn_actors, origins = set(), {}
    preparation_results, preparation_takes = {}, {}
    preparation_workers, discarded, resolved, births = set(), set(), set(), {}
    exit_notifications = deque()
    health_starts = deque()
    health_results, due, reaps = {}, {}, []
    coverage, states = Counter(), set()
    finished = False
    latest = None
    pending_signals = None
    signal_send = None
    produced, publication_reaps = [], []
    generated = None
    outbox = deque()
    final_publication = False
    number = kind = None
    try:
        for number, record in enumerate(records):
            kind = record.get("kind") if isinstance(record, dict) else None
            require(set(record) == {"version", "seq", "kind", "data"}, "invalid record schema")
            require(type(record["version"]) is int and record["version"] == 4, "unknown trace version")
            require(type(record["seq"]) is int and record["seq"] == number, "record sequence gap/reordering")
            kind, data = record["kind"], record["data"]
            require(isinstance(data, dict), "record data must be an object")
            where = f"record {number} ({kind})"
            if kind == "header":
                require(number == 0, f"{where}: duplicate header")
            elif kind == "join":
                receiver = data["receiver"]
                require(receiver not in queues, f"{where}: duplicate receiver")
                joins.add(receiver)
                queues[receiver] = deque()
            elif kind == "leave":
                require(data["receiver"] in joins, f"{where}: unknown receiver departure")
                joins.remove(data["receiver"])
            elif kind == "initial":
                require(initial is None, f"{where}: duplicate initial state")
                config = configuration(data["config"])
                initial = snapshot(data["state"])
                latest, owner = initial, data["receiver"]
                require(owner in joins, f"{where}: supervisor receiver not joined")
            elif kind == "send":
                mid = data["message"]
                require(mid not in sends, f"{where}: duplicate message")
                require(data["sender"] in joins, f"{where}: sender not joined")
                payload = event(data["event"])
                if payload["kind"] == "ServiceExited":
                    require(data["sender"] == owner and exit_notifications
                            and exit_notifications[0] == payload,
                            f"{where}: exit notification has no locally accounted reap")
                    exit_notifications.popleft()
                if payload["kind"] == "StartHealthCheck":
                    require(data["sender"] == owner and health_starts
                            and health_starts[0] == payload,
                            f"{where}: health start has no matching registration")
                    health_starts.popleft()
                actor = data.get("actor", 0)
                if actor in spawn_actors:
                    require(actor in pending, f"{where}: send after spawn_done")
                    raise TraceError(f"{where}: preparation worker must not publish lifecycle inputs")
                if data["sender"] == owner and signal_send is not None:
                    require(payload == signal_send, f"{where}: signal step was not broadcast next")
                    signal_send = None
                elif data["sender"] == owner:
                    require(outbox and outbox[0] == payload,
                            f"{where}: owner publication has no matching ordered output")
                    outbox.popleft()
                run = runs.get(payload["svc"])
                if payload["kind"] == "HealthCheck":
                    actor = data.get("actor", 0)
                    if actor:
                        require(actor in workers, f"{where}: unknown health worker")
                        svc, run, attempt = workers[actor]
                        require(svc == payload["svc"], f"{where}: health worker service mismatch")
                        require(payload["attempt_id"] == attempt,
                                f"{where}: health result changed its worker attempt ID")
                        observation = health_results.get(actor)
                        require(observation is not None and observation["good"] == payload["good"],
                                f"{where}: health result missing")
                        if payload["good"]:
                            require(observation["elapsed_ms"] >= config[svc]["healthy_after_ms"],
                                    f"{where}: health before healthy-after")
                    else:
                        cause = sends.get(data.get("cause"))
                        require(cause is not None and cause["event"]["kind"] == "StartHealthCheck"
                                and cause["event"]["svc"] == payload["svc"],
                                f"{where}: immediate health has no worker-start source")
                        require(payload["attempt_id"] == cause["event"]["attempt_id"],
                                f"{where}: immediate health changed its start attempt ID")
                        require(payload["good"], f"{where}: immediate health must be Healthy")
                        run = cause["run"]
                        require(not config[payload["svc"]]["checks"]
                                and config[payload["svc"]]["healthy_after_ms"] == 0,
                                f"{where}: immediate health bypasses checks/delay")
                sends[mid] = {"event": payload, "run": run, "sender": data["sender"]}
            elif kind == "dispatch":
                mid = data["message"]
                require(mid in sends and mid not in dispatched, f"{where}: unknown/duplicate dispatch")
                dispatched.add(mid)
                receivers = data["receivers"]
                require(len(set(receivers)) == len(receivers), f"{where}: duplicate recipient")
                for receiver in receivers:
                    require(receiver in queues, f"{where}: receiver never joined")
                    queues[receiver].append(mid)
            elif kind == "receive":
                receiver, mid = data["receiver"], data["message"]
                require(receiver in joins and queues[receiver] and queues[receiver][0] == mid,
                        f"{where}: receive violates FIFO/join/dispatch")
                queues[receiver].popleft()
                if receiver == owner:
                    require(not outbox and generated is None,
                            f"{where}: owner receipt before publication batch drained")
                    consumed.add(mid)
            elif kind == "step":
                require(initial is not None and not finished, f"{where}: step outside supervisor lifetime")
                require(not outbox and generated is None and signal_send is None,
                        f"{where}: owner step before publication batch drained")
                inp = event(data["input"], local=True)
                cause = data.get("cause", 0)
                actor = data.get("actor", 0)
                internal = actor in spawn_actors
                local_exit = inp["kind"] == "ReapedExit"
                if local_exit:
                    require(cause == 0 and actor == 0 and reaps and reaps[0] == inp,
                            f"{where}: local exit has no matching observed reap")
                    reaps.pop(0)
                    notification = dict(inp, kind="ServiceExited", pid=0)
                    exit_notifications.append(notification)
                    publication_reaps.append(notification)
                elif internal:
                    require(actor in pending and actor in preparation_takes,
                            f"{where}: owner action has no consumed preparation handoff")
                    require(cause == origins[actor] and runs.get(inp["svc"]) == cause
                            and inp["svc"] == pending[actor],
                            f"{where}: stale preparation or service/origin mismatch")
                    require(actor not in resolved, f"{where}: duplicate preparation resolution")
                    outcome = preparation_takes[actor]
                    require(inp["kind"] in {"SpawnRegistered", "SpawnFailed", "StatusUpdate"},
                            f"{where}: unexpected preparation owner action")
                    if inp["kind"] == "SpawnRegistered":
                        require(outcome == "ready" and actor not in discarded
                                and births.get(actor) == inp["pid"],
                                f"{where}: registration does not match preparation birth")
                        health_starts.append(dict(
                            kind="StartHealthCheck", svc=inp["svc"], target="", pid=0, code=0,
                            good=False, mode="",
                            attempt_id=data["after"]["services"][inp["svc"]]["attempt_id"],
                        ))
                    elif inp["kind"] == "StatusUpdate":
                        require(outcome == "ready" and actor in discarded
                                and inp["target"] == "Failed" and actor not in births,
                                f"{where}: invalid unspawned cancellation")
                    else:
                        require(actor not in births, f"{where}: spawn failure hides a child")
                    resolved.add(actor)
                elif cause:
                    require(inp["kind"] not in LOCAL_EVENTS,
                            f"{where}: registration has no preparation provenance")
                    require(cause in consumed and cause not in handled, f"{where}: missing/duplicate owner receipt")
                    require(sends[cause]["event"] == inp, f"{where}: input differs from sent event")
                    handled.add(cause)
                else:
                    require(inp["kind"] == "Shutdown" and pending_signals is not None,
                            f"{where}: uncorrelated owner input (no pending signals)")
                    signal_send = inp
                if (inp["kind"] == "Run"
                    and data["before"]["services"][inp["svc"]]["status"] == "Initial"
                    and data["after"]["services"][inp["svc"]]["attempt_id"]
                        != data["before"]["services"][inp["svc"]]["attempt_id"]):
                    runs[inp["svc"]] = cause
                for output in data["outputs"]:
                    event(output)
                produced.extend(data["outputs"])
                action = dict(kind="step", before=snapshot(data["before"]),
                              after=snapshot(data["after"]), input=inp, outputs=data["outputs"],
                              signal_count=pending_signals if not cause and not internal and not local_exit else 0,
                              record=number)
                if not cause and not internal and not local_exit:
                    pending_signals = None
                actions.append(action)
                latest = action["after"]
                coverage[inp["kind"]] += 1
                states.update(p["status"] for p in data["after"]["services"].values())
            elif kind == "deadline":
                svc = data["svc"]
                require(config is not None and svc in config and type(data["due"]) is bool,
                        f"{where}: invalid deadline observation")
                require(data["forced"] == (latest["phase"] == "Forceful")
                        and data["wait_secs"] == config[svc]["wait_secs"]
                        and (data["elapsed_secs"] is None) == (not latest["services"][svc]["timer"]),
                        f"{where}: deadline context mismatch")
                expected = latest["services"][svc]["pid"] != 0 and (
                    data["forced"] or (data["elapsed_secs"] is not None
                                       and data["elapsed_secs"] > data["wait_secs"]))
                require(data["due"] == expected, f"{where}: deadline predicate disagrees with observation")
                due[svc] = data["due"]
            elif kind == "generate":
                require(initial is not None and not finished, f"{where}: generation outside lifetime")
                require(not outbox and generated is None,
                        f"{where}: generation before publication batch drained")
                require(pending_signals is None and signal_send is None,
                        f"{where}: signal was not applied/broadcast in its iteration")
                require(not reaps, f"{where}: observed reaps were not accounted locally")
                for emitted in data["events"]:
                    event(emitted)
                actions.append(dict(kind="generate", state=snapshot(data["state"]),
                                    events=data["events"], order=data["order"],
                                    due={n: due.get(n, False) for n in config}, reaps=reaps, record=number))
                generated = data["events"]
                due, reaps = {}, []
            elif kind == "publish":
                require(initial is not None and not outbox and signal_send is None,
                        f"{where}: overlapping owner publication")
                published = data["events"]
                require(isinstance(published, list), f"{where}: invalid owner publication batch")
                for emitted in published:
                    event(emitted)
                if finished:
                    require(not final_publication and not produced and not publication_reaps
                            and generated is None,
                            f"{where}: duplicate/incomplete final owner publication")
                    expected = [dict(kind="Shutdown", svc="", target="", pid=0, code=0,
                                     good=False, mode="Graceful", attempt_id=0)]
                    final_publication = True
                else:
                    require(generated is not None,
                            f"{where}: owner publication has no command generation")
                    # The owner collects every local reap's outputs before appending
                    # any raw exit notifications, then appends generated commands.
                    expected = produced + publication_reaps + generated
                require(published == expected,
                        f"{where}: owner publication batch differs from ordered sources")
                outbox.extend(published)
                produced, publication_reaps, generated = [], [], None
            elif kind == "spawn_request":
                request = data["request"]
                require(request not in spawn_actors and data["svc"] in config, f"{where}: invalid spawn request")
                cause = data["cause"]
                require(cause in consumed and sends[cause]["event"]["kind"] == "Run"
                        and sends[cause]["event"]["svc"] == data["svc"],
                        f"{where}: spawn request has no matching owner Run")
                process = latest["services"][data["svc"]]
                require(process["status"] == "Initial" and process["pid"] == 0
                        and process["attempt_id"] < MAX_ATTEMPT,
                        f"{where}: preparation requested for an ineligible/exhausted attempt")
                pending[request] = data["svc"]
                require(data["receiver"] in joins, f"{where}: spawn receiver not joined")
                spawn_actors.add(request)
                origins[request] = cause
                preparation_workers.add(request)
            elif kind == "preparation_result":
                request = data["request"]
                require(request in preparation_workers and request not in preparation_results
                        and data["actor"] == request and data["cause"] == origins[request],
                        f"{where}: unknown/duplicate preparation result")
                require(data["outcome"] in {"ready", "cancelled", "failed"},
                        f"{where}: invalid preparation result outcome")
                preparation_results[request] = data["outcome"]
            elif kind == "preparation_worker_end":
                request = data["request"]
                require(request in preparation_workers and request in preparation_results
                        and data["actor"] == request and data["cause"] == origins[request],
                        f"{where}: unknown/duplicate preparation worker completion")
                preparation_workers.remove(request)
            elif kind == "preparation_take":
                request = data["request"]
                require(request in pending and request not in preparation_takes
                        and request not in discarded and data["actor"] == request
                        and data["cause"] == origins[request]
                        and runs.get(pending[request]) == origins[request],
                        f"{where}: unknown/duplicate/stale preparation handoff")
                outcome = data["outcome"]
                require(outcome == preparation_results.get(request)
                        or (outcome == "disconnected" and request not in preparation_results),
                        f"{where}: preparation handoff differs from worker result")
                preparation_takes[request] = outcome
            elif kind == "preparation_discard":
                request = data["request"]
                require(request in pending and request not in discarded
                        and request not in births and data["actor"] == request
                        and data["cause"] == origins[request],
                        f"{where}: unknown/duplicate preparation discard")
                discarded.add(request)
            elif kind == "spawn_done":
                request = data["request"]
                require(request in pending and (request in resolved or request in discarded),
                        f"{where}: unknown/unresolved preparation completion")
                del pending[request]
            elif kind == "birth":
                pid, svc = data["pid"], data["svc"]
                request = data["request"]
                require(pending.get(request) == svc and pid > 0
                        and preparation_takes.get(request) == "ready"
                        and request not in discarded and request not in births
                        and data["actor"] == request and data["cause"] == origins[request],
                        f"{where}: birth has no consumed preparation handoff")
                require(pid not in children and svc not in children.values(), f"{where}: overlapping processes")
                children[pid] = svc
                births[request] = pid
            elif kind == "reap":
                pid, svc = data["pid"], data["svc"]
                if pid in children:
                    require(svc == children[pid], f"{where}: managed process reaped before PID registration")
                    del children[pid]
                if svc is not None:
                    require(latest is not None and (pid, svc) in latest["pid_map"],
                            f"{where}: reaped PID differs from registered owner")
                    reaps.append(dict(kind="ReapedExit", svc=svc, target="", pid=pid,
                                      code=data["code"], good=False, mode="", attempt_id=0))
            elif kind == "worker_start":
                worker, svc = data["worker"], data["svc"]
                cause = sends.get(data.get("cause"))
                require(worker not in workers and cause is not None
                        and cause["event"]["kind"] == "StartHealthCheck"
                        and cause["event"]["svc"] == svc, f"{where}: invalid worker origin")
                require(data["attempt_id"] == cause["event"]["attempt_id"],
                        f"{where}: worker start changed its attempt ID")
                workers[worker] = (svc, cause["run"], data["attempt_id"])
            elif kind == "worker_end":
                require(data["worker"] in workers, f"{where}: unknown worker completion")
                del workers[data["worker"]]
            elif kind == "health_result":
                require(data["actor"] in workers, f"{where}: result from inactive worker")
                require(data["attempt_id"] == workers[data["actor"]][2],
                        f"{where}: health observation changed its worker attempt ID")
                health_results[data["actor"]] = data
            elif kind == "signals":
                require(latest is not None and pending_signals is None
                        and type(data["pending"]) is int and data["pending"] > 0,
                        f"{where}: invalid pending signals")
                require(not outbox and generated is None and signal_send is None,
                        f"{where}: signals before publication batch drained")
                if latest["phase"] != "Forceful":
                    pending_signals = data["pending"]
            elif kind == "supervisor_finished":
                require(not finished and initial is not None, f"{where}: invalid completion")
                require(not outbox and generated is None and not produced
                        and not publication_reaps and signal_send is None
                        and pending_signals is None,
                        f"{where}: incomplete owner publication at supervisor finish")
                actions.append(dict(kind="finished", state=snapshot(data["state"]), record=number))
                finished = True
            elif kind == "ambiguous_birth":
                raise TraceError(f"{where}: reap/birth ordering ambiguous; cannot certify process accounting",
                                 stage="uncertifiable")
            elif kind == "complete":
                require(number == len(records) - 1 and finished, f"{where}: invalid completion footer")
                require(not data["pending"] and not data["unreaped"] and not data["workers"]
                        and not data["preparation_workers"] and not preparation_workers
                        and not pending and not children and not workers, f"{where}: completion hides pending/unreaped work")
                require(consumed == handled, f"{where}: not every owner input was handled")
                require(set(sends) == dispatched, f"{where}: undispatched messages remain")
                require(pending_signals is None and signal_send is None,
                        f"{where}: incomplete signal handling")
                require(not reaps and not exit_notifications,
                        f"{where}: incomplete local exit accounting/notification")
                require(not health_starts, f"{where}: missing health-start notification")
                require(final_publication and not outbox and generated is None
                        and not produced and not publication_reaps,
                        f"{where}: incomplete owner publication")
            elif kind not in OBSERVATIONS:
                raise TraceError(f"{where}: unknown record kind")
    except TraceError as error:
        if error.record is None:
            error.record, error.kind = number, kind
        raise
    except (KeyError, TypeError, ValueError, AttributeError) as error:
        raise TraceError(f"record {number} ({kind}): malformed record: {error!r}",
                         record=number, kind=kind) from error
    require(initial is not None and actions, "missing initial state/actions")
    return config, initial, actions, dict(steps=sum(coverage.values()), actions=dict(coverage),
                                          states=sorted(states), complete=True)


def tla(value, depth=0):
    if value is None:
        return '"null"'
    if isinstance(value, bool):
        return "TRUE" if value else "FALSE"
    if isinstance(value, int):
        return f"({value})" if value < 0 else str(value)
    if isinstance(value, str):
        require(all(ord(c) < 128 for c in value), "non-ASCII trace string unsupported")
        return json.dumps(value)
    if isinstance(value, (list, tuple)):
        return "<<" + ", ".join(tla(v, depth + 1) for v in value) + ">>"
    if isinstance(value, (set, frozenset)):
        return "{" + ", ".join(sorted(tla(v, depth + 1) for v in value)) + "}"
    if isinstance(value, dict):
        key = f"key{depth}"
        if not value:
            return f"[{key} \\in {{}} |-> 0]"
        keys = ", ".join(tla(k, depth + 1) for k in value)
        cases = " [] ".join(f"{key} = {tla(k, depth + 1)} -> {tla(v, depth + 1)}"
                           for k, v in value.items())
        return f"[{key} \\in {{{keys}}} |-> CASE {cases}]"
    raise TraceError(f"unsupported trace value type: {type(value).__name__}")


def load(path):
    records = []
    for number, line in enumerate(Path(path).read_text().splitlines()):
        try:
            records.append(json.loads(line, object_pairs_hook=unique_object))
        except (ValueError, TraceError) as error:
            raise TraceError(f"record {number}: invalid JSON line: {error}",
                             record=number, stage="parse") from error
    return records


def attempt_encoding(values):
    ids = {0, MAX_ATTEMPT}

    def collect(value):
        if isinstance(value, dict):
            for key, item in value.items():
                if key == "attempt_id":
                    require(type(item) is int and 0 <= item <= MAX_ATTEMPT, "invalid attempt ID")
                    ids.add(item)
                else:
                    collect(item)
        elif isinstance(value, (list, tuple)):
            for item in value:
                collect(item)

    collect(values)
    # TLC integers are bounded. Including each observed successor preserves +1
    # as well as identity equality; gaps cannot hide a skipped/wrapped attempt.
    ids.update(value + 1 for value in tuple(ids) if value < MAX_ATTEMPT)
    return {value: rank for rank, value in enumerate(sorted(ids))}


def encode_attempts(value, encoding):
    if isinstance(value, dict):
        return {key: encoding[item] if key == "attempt_id" else encode_attempts(item, encoding)
                for key, item in value.items()}
    if isinstance(value, list):
        return [encode_attempts(item, encoding) for item in value]
    if isinstance(value, tuple):
        return tuple(encode_attempts(item, encoding) for item in value)
    return value


def bundle(output, config, initial, actions, extra_attempt_values=()):
    encoding = attempt_encoding([initial, actions, extra_attempt_values])
    data = "---------------- MODULE TraceData ----------------\nEXTENDS Integers\n"
    data += f"AttemptMaximum == {encoding[MAX_ATTEMPT]}\n"
    data += f"AttemptEncoding == {tla({str(k): v for k, v in encoding.items()})}\n"
    data += f"Config == {tla(config)}\nInitial == {tla(encode_attempts(initial, encoding))}\n"
    data += f"Actions == {tla(encode_attempts(actions, encoding))}\n"
    data += "====================================================\n"
    (output / "TraceData.tla").write_text(data)
    for name in ("ProcessRules.tla", "TraceValidation.tla"):
        shutil.copyfile(ROOT / "specs" / name, output / name)
    shutil.copyfile(ROOT / "specs/configs/TraceValidation.cfg", output / "TraceValidation.cfg")
    return encoding


def run_tlc(output):
    command = ["java", "-Xmx1g", "-XX:+UseParallelGC", "-cp", str(checker()),
               "tlc2.TLC", "-noGenerateSpecTE", "-workers", "1",
               "-metadir", str(output.resolve() / "states"), "TraceValidation.tla"]
    try:
        result = subprocess.run(command, cwd=output, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired as error:
        partial = [part.decode(errors="replace") if isinstance(part, bytes) else part or ""
                   for part in (error.stdout, error.stderr)]
        (output / "tlc.log").write_text("".join(partial) + "\n[TLC timed out; partial output]\n")
        raise
    log = result.stdout + result.stderr
    (output / "tlc.log").write_text(log)
    return result.returncode, log


def diagnose(returncode, log, actions, initial_record):
    """Map TLC's Accepted counterexample to the trace record that falsified it.

    Init sets index = 1 and Advance checks Actions[index] while setting
    index' = index + 1, so a final state with index = N was produced by the
    1-based Actions[N - 1], i.e. Python actions[N - 2]. index = 1 is Init.
    """
    if returncode == 0 and "No error has been found" in log:
        return None
    indexes = re.findall(r"^/\\ index = (\d+)\s*$", log, re.MULTILINE)
    if "Invariant Accepted is violated" in log and indexes:
        last = int(indexes[-1])
        if last == 1 or "violated by the initial state" in log:
            return dict(stage="initial", record=initial_record, kind="initial", action=None,
                        input_kind=None, reason=f"TLC rejected the initial state "
                        f"(record {initial_record}, initial)")
        position = last - 2
        if 0 <= position < len(actions):
            action = actions[position]
            input_kind = action["input"]["kind"] if action["kind"] == "step" else None
            label = action["kind"] + (f" {input_kind}" if input_kind else "")
            return dict(stage="action", record=action["record"], kind=action["kind"],
                        action=position, input_kind=input_kind,
                        reason=f"TLC rejected record {action['record']} ({label}) "
                        f"at projected action {position}")
    return dict(stage="tlc", record=None, kind=None, action=None, input_kind=None,
                reason=f"TLC failed without a mappable Accepted counterexample (exit {returncode})")


def prefix_coverage(actions):
    inputs = Counter(a["input"]["kind"] for a in actions if a["kind"] == "step")
    return dict(actions=len(actions), steps=sum(inputs.values()), inputs=dict(inputs))


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def validate(path, output):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    path = Path(path).resolve()
    shutil.copyfile(path, output / "input.ndjson")
    result = dict(status="rejected", input="input.ndjson",
                  input_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                  stage=None, reason=None, record=None, kind=None, action=None, input_kind=None)
    try:
        try:
            records = load(path)
            result["stage"] = "project"
            config, initial, actions, summary = project(records)
            result["coverage"] = summary
            result["stage"] = "tooling"
            bundle(output, config, initial, actions)
            returncode, log = run_tlc(output)
        except (KeyError, TypeError, ValueError, OSError, subprocess.TimeoutExpired) as error:
            raise TraceError(f"malformed trace or tooling failure: {error}",
                             stage="tooling") from error
        result["tlc_exit"] = returncode
        initial_record = next(i for i, r in enumerate(records) if r["kind"] == "initial")
        found = diagnose(returncode, log, actions, initial_record)
        if found is not None:
            failing = found["action"] if found["action"] is not None else 0
            result["accepted_prefix"] = prefix_coverage(actions[:failing])
            result.update(found)
            raise TraceError(f"{found['reason']} (exit {returncode}); see {output / 'tlc.log'}",
                             record=found["record"], kind=found["kind"], stage=found["stage"])
    except TraceError as error:
        error.stage = error.stage or result["stage"]
        if error.stage in INCONCLUSIVE:
            result["status"] = "error"
        result.update(stage=error.stage, reason=str(error), record=error.record, kind=error.kind)
        error.result = result
        write_json(output / "result.json", result)
        raise
    result.update(status="accepted", stage="accepted", accepted_prefix=prefix_coverage(actions))
    write_json(output / "result.json", result)
    write_json(output / "summary.json", summary)
    return summary


CERTIFICATION = ("prefix replay certification only: the projected TLA+ Actions prefix through "
                 "the first failing action reproduces the same Accepted violation at the same "
                 "record; not a minimal raw causal trace and not an OS replay")


def minimize(path, output):
    """Conservatively shrink a TLC Accepted action failure to its projected prefix."""
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    report = dict(status="failed", certification=CERTIFICATION)
    try:
        try:
            validate(path, output / "full")
        except TraceError as error:
            full = error
        else:
            raise TraceError("trace accepted by TLC; nothing to minimize", stage="inapplicable")
        if full.stage != "action":
            raise TraceError(f"cannot minimize {full.stage} failure: {full}", record=full.record,
                             kind=full.kind, stage=full.stage)
        records = load(output / "full" / "input.ndjson")
        config, initial, actions, _ = project(records)
        position = full.result["action"]
        prefix = actions[:position + 1]
        small = output / "minimized"
        small.mkdir()
        bundle(small, config, initial, prefix)
        returncode, log = run_tlc(small)
        initial_record = next(i for i, r in enumerate(records) if r["kind"] == "initial")
        found = diagnose(returncode, log, prefix, initial_record)
        require(found is not None and found["stage"] == "action"
                and (found["record"], found["action"]) == (full.record, position),
                f"minimized prefix did not reproduce the Accepted violation at record "
                f"{full.record}: {found}")
        report.update(status="reproduced", record=full.record, kind=full.kind,
                      input_kind=found["input_kind"], action=position,
                      full_actions=len(actions), minimized_actions=len(prefix),
                      tlc_exit=returncode, full="full", minimized="minimized")
    except TraceError as error:
        report.update(reason=str(error), stage=error.stage, record=error.record, kind=error.kind)
        write_json(output / "minimize.json", report)
        raise
    except (KeyError, TypeError, ValueError, OSError, subprocess.TimeoutExpired) as error:
        report.update(reason=f"tooling failure: {error}", stage="tooling")
        write_json(output / "minimize.json", report)
        raise TraceError(report["reason"], stage="tooling") from error
    write_json(output / "minimize.json", report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(validate(args.trace, args.out), indent=2))
    except TraceError as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
