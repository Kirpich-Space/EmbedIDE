#!/usr/bin/env python3
"""Measure warm-cache headless doctor launches; does not claim cold startup."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=30)
    options = parser.parse_args()
    if options.samples < 2:
        parser.error("--samples must be at least 2")
    binary = options.binary.resolve(strict=True)
    environment = os.environ.copy()
    environment.pop("DISPLAY", None)
    environment.pop("WAYLAND_DISPLAY", None)

    def launch():
        start = time.perf_counter_ns()
        result = subprocess.run(
            [str(binary), "doctor", "--json"],
            env=environment, capture_output=True, timeout=30, check=True,
        )
        elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
        if json.loads(result.stdout).get("ok") is not True:
            raise RuntimeError("doctor returned an unsuccessful result")
        return elapsed_ms

    launch()  # Explicit warm-up; no page-cache flushing or privileged operations.
    samples = sorted(launch() for _ in range(options.samples))
    report = {
        "scenario": "doctor subprocess including PATH inventory; warm filesystem cache",
        "binary": str(binary),
        "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "binary_bytes": binary.stat().st_size,
        "platform": platform.platform(),
        "machine": platform.machine(),
        "samples": options.samples,
        "wall_ms": {
            "min": samples[0],
            "median": statistics.median(samples),
            "p95": samples[math.ceil(len(samples) * 0.95) - 1],
            "max": samples[-1],
        },
        "cold_start_measured": False,
    }
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
