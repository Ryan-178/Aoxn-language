"""Compare two release compilers without clang/cache. Run from any directory.

python tools/compiler_perf.py --before target/compiler-perf/baseline.exe \
    --after target/release/aoxn.exe --output target/compiler-perf/final.json
Build and preserve the baseline at 82cffcd before changing compiler sources.
No cargo builds/tests should compete with the timed runs.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def invoke(exe, source, timed):
    env = dict(os.environ, AOXN_NO_CACHE="1")
    env.pop("AOXN_TIME", None)
    if timed:
        env["AOXN_TIME"] = "1"
    start = time.perf_counter()
    p = subprocess.run([str(exe), "c", str(source)], cwd=ROOT, env=env,
                       stdout=subprocess.DEVNULL if timed else subprocess.PIPE,
                       stderr=subprocess.PIPE)
    wall = (time.perf_counter() - start) * 1000
    if not timed:
        return p
    if p.returncode:
        raise RuntimeError(p.stderr.decode("utf-8", errors="replace"))
    phases = {}
    for label, number, unit in re.findall(r"\[time\] (.*): ([\d.]+)(ns|µs|us|ms|s)",
                                          p.stderr.decode("utf-8")):
        label = label.split(" ")[0]
        phases[label] = phases.get(label, 0) + float(number) * {
            "ns": 1e-6, "µs": .001, "us": .001, "ms": 1, "s": 1000}[unit]
    return {"wall": wall, **phases, "stderr": p.stderr.decode("utf-8")}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--before", type=Path, required=True)
    ap.add_argument("--after", type=Path, required=True)
    ap.add_argument("--output", type=Path, required=True)
    args = ap.parse_args()
    exes = {"before": args.before.resolve(), "after": args.after.resolve()}
    output = args.output.resolve()
    inputs = output.parent / "inputs"
    inputs.mkdir(parents=True, exist_ok=True)
    samples = []
    for n in (200, 800, 1600):
        source = "".join(f"struct S{i}:\n    child: S{i+1}\n" for i in range(n-1))
        source += f"struct S{n-1}:\n    value: int\ndef main() -> int:\n    return 0\n"
        p = inputs / f"struct-chain-{n}.ax"
        p.write_text(source, encoding="utf-8")
        samples.append(p)
    samples += [ROOT / "selfhost/driver_self_demo.ax", ROOT / "examples/ui_gallery.ax",
                ROOT / "examples/stdlib_demo.ax"]
    report = {"baseline_commit": "82cffcd", "unit": "ms", "samples": {}, "equivalence": []}
    for source in samples:
        key = str(source.relative_to(ROOT))
        rows = {k: [] for k in exes}
        for round_number in range(6):
            order = ["before", "after"] if round_number % 2 == 0 else ["after", "before"]
            for variant in order:
                row = invoke(exes[variant], source, True)
                if round_number:  # discard both warmups
                    rows[variant].append(row)
        summary = {}
        for variant, values in rows.items():
            summary[variant] = {phase: {"min": min(v[phase] for v in values),
                                       "median": statistics.median(v[phase] for v in values)}
                                for phase in values[0] if phase != "stderr"}
        report["samples"][key] = {"raw": rows, "summary": summary}
        print(key, json.dumps(summary), flush=True)
    # Cross-process byte comparisons include emitted C, warning/error text, status.
    sources = list(ROOT.glob("examples/*.ax")) + list(ROOT.glob("selfhost/*demo.ax")) + samples[:3]
    invalid = {
        "cycle-tail": "struct Head:\n    tail: Loop\nstruct Loop:\n    back: Loop\ndef main() -> int:\n    return 0\n",
        "duplicate-field": "struct S:\n    x: int\n    x: bool\ndef main() -> int:\n    return 0\n",
        "unknown-type": "struct S:\n    x: Unknown\ndef main() -> int:\n    return 0\n",
        "strict-type": 'def main() -> int:\n    return "bad"\n',
        "parse": "def main( -> int:\n    return 0\n",
    }
    for name, text in invalid.items():
        p = inputs / f"invalid-{name}.ax"
        p.write_text(text, encoding="utf-8")
        sources.append(p)
    for source in sources:
        a, b = [invoke(exes[k], source, False) for k in ("before", "after")]
        equal = (a.returncode, a.stdout, a.stderr) == (b.returncode, b.stdout, b.stderr)
        report["equivalence"].append({"source": str(source.relative_to(ROOT)), "equal": equal,
                                      "status": [a.returncode, b.returncode],
                                      "stdout_sha256": hashlib.sha256(a.stdout).hexdigest(),
                                      "stderr_sha256": hashlib.sha256(a.stderr).hexdigest()})
        if not equal:
            raise AssertionError(f"baseline/modified output differs: {source}")
    output.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"Byte-identical outcomes: {len(sources)}; raw report: {output}")


if __name__ == "__main__":
    main()
