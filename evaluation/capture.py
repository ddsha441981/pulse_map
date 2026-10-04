#!/usr/bin/env python3
"""Capture reproducibility metadata and raw benchmark stdout (run in evaluation/)."""
import argparse
import json
import pathlib
import platform
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("label")
parser.add_argument("--smoke", action="store_true")
args = parser.parse_args()
if not args.label.replace("-", "").replace("_", "").isalnum():
    parser.error("label must be alphanumeric with hyphens/underscores")
root = pathlib.Path(__file__).resolve().parent
out = root / "results" / args.label
out.mkdir(parents=True, exist_ok=False)


def command(*cmd):
    return subprocess.check_output(cmd, cwd=root, text=True)


metadata = {
    "revision": command("git", "rev-parse", "HEAD").strip(),
    "status": command("git", "status", "--short"),
    "rustc": command("rustc", "-Vv"),
    "platform": platform.platform(),
    "cpu": command("lscpu"),
    "smoke": args.smoke,
    "packages": [
        {key: package[key] for key in ("name", "version", "source", "manifest_path")}
        for package in json.loads(command("cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", "x86_64-unknown-linux-gnu"))["packages"]
    ],
}
(out / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
subprocess.run(["cargo", "build", "--release", "--locked"], cwd=root, check=True)
for scenario in ("hitrate", "throughput", "memory"):
    cmd = [str(root / "target/release/pulsemap-evaluation"), scenario]
    if args.smoke:
        cmd.append("--smoke")
    print(f"Capturing {scenario} -> {out}", flush=True)
    with (out / f"{scenario}.csv").open("w") as stream:
        subprocess.run(cmd, cwd=root, stdout=stream, check=True)
print(f"Saved {out}")
