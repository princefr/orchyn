#!/usr/bin/env python3
"""Combine bench/results/*.json into one markdown table.

A fixture counts as a *valid catch* only when the manifest fails on every
buggy run AND passes on every clean run; a manifest that also fails on the
clean app has not demonstrated anything about the bug.
"""
import json, pathlib, sys

root = pathlib.Path(__file__).parent / "results"
order = ["reference", "haiku-spec-1", "haiku-spec-2", "haiku-explore-1", "haiku-explore-2"]
files = {p.stem: json.loads(p.read_text()) for p in root.glob("*.json")}
labels = [l for l in order if l in files] + sorted(set(files) - set(order))
fixtures = [r["name"] for r in files[labels[0]]["results"]]

def cell(r):
    if r["manifest"] is None:
        return "n/a"
    if r["false_positive"]:
        return "✗ clean fails"
    if r["detected"]:
        return "✓" + (" (flaky)" if r["flaky"] else "")
    return "missed"

out = ["| fixture | class | " + " | ".join(labels) + " |",
       "|---|---|" + "---|" * len(labels)]
for name in fixtures:
    rows = {l: next(r for r in files[l]["results"] if r["name"] == name) for l in labels}
    out.append(f"| {name} | {rows[labels[0]]['bug_class']} | " +
               " | ".join(cell(rows[l]) for l in labels) + " |")
valid = {l: sum(1 for r in files[l]["results"]
                if r["detected"] and not r["false_positive"]) for l in labels}
fps = {l: sum(1 for r in files[l]["results"] if r["false_positive"]) for l in labels}
flaky = {l: sum(1 for r in files[l]["results"] if r["flaky"]) for l in labels}
runs = {l: files[l]["runs_per_variant"] for l in labels}
ms = {l: files[l]["mean_run_ms"] for l in labels}
out.append("| **valid catches** | | " + " | ".join(f"**{valid[l]}/{len(fixtures)}**" for l in labels) + " |")
out.append("| false positives | | " + " | ".join(str(fps[l]) for l in labels) + " |")
out.append("| flaky fixtures | | " + " | ".join(str(flaky[l]) for l in labels) + " |")
out.append("| runs per variant | | " + " | ".join(str(runs[l]) for l in labels) + " |")
out.append("| mean run (ms) | | " + " | ".join(str(ms[l]) for l in labels) + " |")
print("\n".join(out))
