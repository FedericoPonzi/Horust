"""Seeded schedule/real-process campaigns; failures remain evidence, not fixes."""

import argparse
from collections import Counter
import ctypes
import hashlib
import json
import os
from pathlib import Path
import random
import shlex
import signal
import subprocess
import sys
import tempfile
import time

from validate import INCONCLUSIVE, ROOT, TraceError, checker, validate


def positive(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def trace_status(error):
    return "inconclusive" if error.stage in INCONCLUSIVE else "rejected"


def scenario(seed):
    rng = random.Random(seed)
    services = []
    count = 1 if seed == 0 else rng.randint(1, 3)
    for index in range(count):
        mode = "sleep" if seed == 0 else rng.choice(("sleep", "immediate", "ignore-term", "flap"))
        if seed == 2 and index == 0:
            mode = "ignore-term"
        if seed == 4 and index == 0:
            mode = "immediate"
        services.append(dict(
            name=f"svc{index}", mode=mode,
            duration=8, code=rng.choice((0, 1, 7)),
            strategy="never" if seed == 0 else rng.choice(("never", "always", "on-failure")),
            attempts=rng.randint(1, 2),
            failure="ignore" if seed == 0 else rng.choice(("ignore", "shutdown", "kill-dependents")),
            start_after=[f"svc{index - 1}"] if index > 0 and rng.choice((True, False)) else [],
            shutdown_after=[f"svc{index + 1}"] if index + 1 < count and rng.choice((True, False)) else [],
            die_if_failed=["svc0"] if index > 0 and rng.choice((True, False)) else [],
            delay_ms=5000 if seed == 6 else (0 if seed == 0 else rng.choice((0, 50, 200))),
            healthy_ms=0 if seed == 0 else rng.choice((0, 50, 100)),
            max_failed=rng.choice((0, 1, 3)),
            successful_codes=[0, 7] if rng.choice((True, False)) else [0],
            missing_command=seed == 3 and index == 0,
        ))
    return dict(seed=seed, services=services, signal_when_running=True,
                escalate=seed != 0 and (seed == 2 or rng.choice((True, False))))


def write_services(case, plan):
    folder = case / "services"
    folder.mkdir()
    for service in plan["services"]:
        health = case / (service["name"] + ".healthy")
        command = shlex.join([sys.executable, str(ROOT / "tools/formal/child.py"), service["mode"],
                              "--duration", str(service["duration"]), "--code", str(service["code"]),
                              "--health", str(health)])
        if service["missing_command"]:
            command = "/horust-formal-command-does-not-exist"
        text = f'name = {json.dumps(service["name"])}\ncommand = {json.dumps(command)}\n'
        text += f'start-after = {json.dumps(service["start_after"])}\n'
        text += f'shutdown-after = {json.dumps(service["shutdown_after"])}\n'
        text += f'start-delay = "{service["delay_ms"]}ms"\n'
        text += f'[restart]\nstrategy = "{service["strategy"]}"\nattempts = {service["attempts"]}\nbackoff = "50ms"\n'
        text += f'[healthiness]\nhealthy-after = "{service["healthy_ms"]}ms"\nmax-failed = {service["max_failed"]}\n'
        if service["mode"] == "flap":
            text += f'file-path = {json.dumps(str(health))}\n'
        text += f'[failure]\nstrategy = "{service["failure"]}"\nsuccessful-exit-code = {json.dumps(service["successful_codes"])}\n'
        text += f'[termination]\nwait = "100ms"\ndie-if-failed = {json.dumps(service["die_if_failed"])}\n'
        (folder / (service["name"] + ".toml")).write_text(text)
    return folder


def process_identity(pid):
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()
        return int(fields[1]), fields[19]
    except (FileNotFoundError, ProcessLookupError):
        return None


def subreaper():
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(36, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "cannot enable local campaign child subreaping")


def cleanup_children(identities):
    failures = []
    for pid, identity in identities.items():
        current = process_identity(pid)
        if current is None or current[1] != identity:
            continue
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            continue
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            try:
                reaped, _ = os.waitpid(pid, os.WNOHANG)
            except ChildProcessError:
                reaped = 0
            if reaped == pid or process_identity(pid) is None:
                break
            time.sleep(0.01)
        else:
            failures.append(pid)
    if failures:
        raise RuntimeError(f"could not reap exact campaign children: {failures}")


def finish_case(child, trace, children):
    try:
        try:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=3)
        finally:
            if trace.exists():
                for line in trace.read_text().splitlines(keepends=True):
                    if not line.endswith("\n"):
                        continue
                    record = json.loads(line)
                    if record["kind"] == "birth":
                        pid = record["data"]["pid"]
                        identity = process_identity(pid)
                        if identity is not None and identity[0] == os.getpid():
                            children[pid] = identity[1]
    finally:
        cleanup_children(children)


def real_case(case, seed):
    subreaper()
    plan = scenario(seed)
    (case / "scenario.json").write_text(json.dumps(plan, indent=2) + "\n")
    folder = write_services(case, plan)
    trace = case / "trace.ndjson"
    env = dict(os.environ, HORUST_FORMAL_TRACE=str(trace))
    command = [str(ROOT / "target/debug/horust"), "--services-path", str(folder),
               "--uds-folder-path", str(case)]
    children, sent = {}, []
    running = False
    offset = 0
    partial = ""
    timed_out = False
    with (case / "run.log").open("wb") as log:
        child = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
        started = time.monotonic()
        try:
            while child.poll() is None:
                if trace.exists():
                    with trace.open() as stream:
                        stream.seek(offset)
                        chunk = stream.read()
                        offset = stream.tell()
                    lines = (partial + chunk).split("\n")
                    partial = lines.pop()
                    for line in lines:
                        record = json.loads(line)
                        if record["kind"] == "birth":
                            pid = record["data"]["pid"]
                            identity = process_identity(pid)
                            if identity is not None and identity[0] in (child.pid, os.getpid()):
                                children[pid] = identity[1]
                        elif record["kind"] == "step":
                            running |= any(p["status"] == "Running"
                                           for p in record["data"]["after"]["services"].values())
                elapsed = time.monotonic() - started
                if not sent and (running or elapsed > 2.5):
                    try:
                        child.send_signal(signal.SIGTERM)
                        sent.append(dict(signal="SIGTERM", elapsed=elapsed, pid=child.pid))
                    except ProcessLookupError:
                        pass
                elif sent and plan["escalate"] and len(sent) == 1 and elapsed - sent[0]["elapsed"] > 0.6:
                    try:
                        child.send_signal(signal.SIGINT)
                        sent.append(dict(signal="SIGINT", elapsed=elapsed, pid=child.pid))
                    except ProcessLookupError:
                        pass
                if elapsed > 15:
                    timed_out = True
                    break
                time.sleep(0.02)
        finally:
            finish_case(child, trace, children)
    (case / "signals.json").write_text(json.dumps(sent, indent=2) + "\n")
    if timed_out:
        raise RuntimeError("real-process watchdog expired; partial trace retained")
    if child.returncode != 0:
        raise RuntimeError(f"instrumented Horust exited {child.returncode}; see run.log")


def schedule_case(case, seed, depth):
    env = dict(os.environ, HORUST_FORMAL_TRACE=str(case / "trace.ndjson"),
               HORUST_FORMAL_SEED=str(seed), HORUST_FORMAL_DEPTH=str(depth))
    command = ["cargo", "test", "-p", "horust", "--lib", "--features", "formal-trace",
               "--quiet", "--", "--ignored", "--exact", "horust::supervisor::formal_tests::formal_schedule"]
    with (case / "run.log").open("wb") as log:
        subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT,
                       check=True, timeout=90)


def source_version():
    digest = hashlib.sha256()
    paths = [ROOT / "Cargo.lock", ROOT / "Cargo.toml", ROOT / "horust/Cargo.toml",
             ROOT / "commands/Cargo.toml"]
    for folder, patterns in ((ROOT / "horust/src", ("*.rs",)),
                             (ROOT / "commands", ("*.rs", "*.proto")),
                             (ROOT / "specs", ("*.tla", "*.cfg")),
                             (ROOT / "tools/formal", ("*.py",))):
        for pattern in patterns:
            paths.extend(folder.rglob(pattern))
    for path in sorted(paths):
        digest.update(str(path.relative_to(ROOT)).encode())
        digest.update(path.read_bytes())
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("schedule", "process"))
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--count", type=positive, default=8)
    parser.add_argument("--depth", type=positive, default=160)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    if args.seed < 0 or args.seed + args.count > 2**64:
        parser.error("seed range must fit unsigned 64-bit integers")
    checker()
    if args.mode == "process":
        subprocess.run(["cargo", "build", "-p", "horust", "--features", "formal-trace", "--quiet"],
                       cwd=ROOT, check=True, timeout=180)
    root = args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix=f"horust-{args.mode}-"))
    if args.out:
        root.mkdir(parents=True, exist_ok=False)
    manifest = dict(mode=args.mode, seed=args.seed, count=args.count, depth=args.depth,
                    source_sha256=source_version(), results=[])
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    for seed in range(args.seed, args.seed + args.count):
        case = root / f"seed-{seed}"
        case.mkdir()
        result = dict(seed=seed)
        try:
            if args.mode == "schedule":
                schedule_case(case, seed, args.depth)
            else:
                real_case(case, seed)
            result["coverage"] = validate(case / "trace.ndjson", case / "check")
            result["status"] = "accepted"
        except TraceError as error:
            result.update(status=trace_status(error), reason=str(error))
        except (RuntimeError, OSError, ValueError,
                subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            result.update(status="inconclusive", reason=str(error))
        manifest["results"].append(result)
        (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"{args.mode} seed {seed}: {result['status']}" +
              (f": {result['reason']}" if "reason" in result else ""), flush=True)
    counts = Counter(result["status"] for result in manifest["results"])
    print(f"Artifacts: {root}\nResults: {dict(counts)}")
    return 0 if counts["accepted"] == args.count else 1


if __name__ == "__main__":
    raise SystemExit(main())
