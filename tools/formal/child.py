"""Bounded leaf process used by local Horust fuzz campaigns (no subprocesses)."""

import argparse
from pathlib import Path
import signal
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("sleep", "immediate", "ignore-term", "flap"))
    parser.add_argument("--duration", type=float, default=8)
    parser.add_argument("--code", type=int, default=0)
    parser.add_argument("--health", type=Path)
    args = parser.parse_args()
    if not 0 <= args.duration <= 20:
        parser.error("duration must be between 0 and 20 seconds")
    if args.mode == "ignore-term":
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
    if args.mode == "immediate":
        return args.code
    deadline = time.monotonic() + args.duration
    present = False
    while time.monotonic() < deadline:
        if args.mode == "flap" and args.health is not None:
            present = not present
            if present:
                args.health.touch()
            else:
                args.health.unlink(missing_ok=True)
        time.sleep(0.15 if args.mode == "flap" else 0.05)
    return args.code


if __name__ == "__main__":
    raise SystemExit(main())
