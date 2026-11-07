"""Run local models, trace checks, tests and fuzz campaigns with pinned TLC."""

import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

from validate import INCONCLUSIVE, ROOT, TraceError, checker, minimize, validate

SPECS = ROOT / "specs"
CONFIGS = SPECS / "configs"
FORMAL = ROOT / "tools/formal"
EXPECTED = {
    "HorustCycle": (13, "Temporal property StartupProgress was violated"),
    "HorustShutdownCycle": (13, "Temporal property ShutdownProgress was violated"),
}


def configs():
    return sorted(p.stem for p in CONFIGS.glob("*.cfg") if p.stem != "TraceValidation")


def module_for(name):
    for prefix, module in (("Extended", "ProcessExtended.tla"), ("Horust", "Horust.tla"),
                           ("Process", "Process.tla"), ("Supervisor", "SupervisorMC.tla")):
        if name.startswith(prefix):
            return module
    raise ValueError(f"no model module for config {name}")


def fresh(out, prefix):
    if out is None:
        return Path(tempfile.mkdtemp(prefix=prefix))
    out = out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    return out


def classify(name, returncode, log):
    expected, marker = EXPECTED.get(name, (0, "No error has been found"))
    distinct = re.findall(r"(\d+) distinct states found", log)
    return dict(name=name, module=module_for(name), expected_exit=expected, exit=returncode,
                distinct_states=int(distinct[-1]) if distinct else None,
                status="ok" if returncode == expected and marker in log else "unexpected")


def run_models(root, names):
    jar = checker()
    results = []
    for name in names:
        case = root / name
        case.mkdir()
        for spec in SPECS.glob("*.tla"):
            shutil.copyfile(spec, case / spec.name)
        shutil.copyfile(CONFIGS / f"{name}.cfg", case / f"{name}.cfg")
        command = ["java", "-Xmx2g", "-XX:+UseParallelGC", "-cp", str(jar), "tlc2.TLC",
                   "-noGenerateSpecTE", "-workers", "1", "-config", f"{name}.cfg",
                   "-metadir", str(case / "states"), module_for(name)]
        run = subprocess.run(command, cwd=case, capture_output=True, text=True, timeout=1800)
        log = run.stdout + run.stderr
        (case / "tlc.log").write_text(log)
        result = classify(name, run.returncode, log)
        results.append(result)
        print(f"{name}: {result['status']} (exit {result['exit']}, expected "
              f"{result['expected_exit']}, {result['distinct_states']} distinct states)", flush=True)
    (root / "result.json").write_text(json.dumps(dict(jar=str(jar), models=results), indent=2) + "\n")
    bad = [r["name"] for r in results if r["status"] != "ok"]
    print(f"Artifacts: {root}\n" + (f"UNEXPECTED: {bad}" if bad else
          f"All {len(results)} configs matched expectations"))
    return 1 if bad else 0


def parser():
    top = argparse.ArgumentParser(description=__doc__,
                                  formatter_class=argparse.RawDescriptionHelpFormatter)
    modes = top.add_subparsers(dest="mode", required=True)
    models = modes.add_parser("models", help="TLC matrix (both dependency-cycle cases must fail)")
    models.add_argument("--only", nargs="+", choices=configs())
    models.add_argument("--out", type=Path)
    modes.add_parser("test", help="build instrumented horust, run Python tool tests")
    for name in ("validate", "minimize"):
        mode = modes.add_parser(name, help=f"{name} one NDJSON trace")
        mode.add_argument("trace", type=Path)
        mode.add_argument("--out", type=Path)
    fuzz = modes.add_parser("fuzz", help="delegate to fuzz.py campaigns")
    fuzz.add_argument("args", nargs=argparse.REMAINDER)
    replay = modes.add_parser("replay", help="rerun one fuzz seed")
    replay.add_argument("kind", choices=("schedule", "process"))
    replay.add_argument("seed", type=int)
    replay.add_argument("--depth", type=int)
    replay.add_argument("--out", type=Path)
    return top


def fuzz_command(argv):
    args = parser().parse_args(argv)
    command = [sys.executable, str(FORMAL / "fuzz.py")]
    if args.mode == "fuzz":
        return command + args.args
    command += [args.kind, "--seed", str(args.seed), "--count", "1"]
    if args.depth is not None:
        command += ["--depth", str(args.depth)]
    if args.out is not None:
        command += ["--out", str(args.out)]
    return command


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    args = parser().parse_args(argv)
    if args.mode == "models":
        return run_models(fresh(args.out, "horust-formal-models-"), args.only or configs())
    if args.mode == "test":
        subprocess.run(["cargo", "build", "-p", "horust", "--features", "formal-trace", "--quiet"],
                       cwd=ROOT, check=True, timeout=600)
        return subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", str(FORMAL),
                               "-p", "test_*.py"], cwd=FORMAL).returncode
    if args.mode in ("fuzz", "replay"):
        return subprocess.run(fuzz_command(argv), cwd=ROOT).returncode
    out = args.out.resolve() if args.out else fresh(None, f"horust-formal-{args.mode}-") / "check"
    try:
        if args.mode == "validate":
            print(json.dumps(validate(args.trace, out), indent=2))
            print(f"accepted; artifacts: {out}")
        else:
            report = minimize(args.trace, out)
            print(f"reproduced Accepted violation at record {report['record']} ({report['kind']}) "
                  f"with {report['minimized_actions']} of {report['full_actions']} projected "
                  f"actions; {report['certification']}; artifacts: {out}")
    except TraceError as error:
        label = "ERROR" if error.stage in INCONCLUSIVE else "REJECTED"
        if args.mode == "minimize":
            label = "minimize FAILED" + (" (ERROR)" if label == "ERROR" else "")
        print(f"{label}: {error}\nartifacts: {out}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
