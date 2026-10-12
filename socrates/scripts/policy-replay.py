#!/usr/bin/env python3
"""Replay policy scenarios x arms x trials through headless `claude -p`, score each run and print pass rates."""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


def policy_for_arm(text: str, minus: list[str]) -> str:
    lines = text.splitlines(keepends=True)
    kept = []
    hit = {prefix: 0 for prefix in minus}
    for line in lines:
        body = line.lstrip()[2:] if line.lstrip().startswith("- ") else None
        match = next((p for p in minus if body is not None and body.startswith(p)), None)
        if match is None:
            kept.append(line)
        else:
            hit[match] += 1
    missing = [p for p, n in hit.items() if n == 0]
    if missing:
        raise SystemExit(f"policy-replay: rule prefix matches no rule: {missing}")
    return "".join(kept)


def events(stdout: str) -> list[dict]:
    out = []
    for line in stdout.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return out


def final_text(evs: list[dict]) -> str:
    for ev in reversed(evs):
        if ev.get("type") == "result" and isinstance(ev.get("result"), str):
            return ev["result"]
    return ""


def tool_calls(evs: list[dict]) -> list[dict]:
    calls = []
    for ev in evs:
        if ev.get("type") != "assistant":
            continue
        for block in ev.get("message", {}).get("content", []) or []:
            if isinstance(block, dict) and block.get("type") == "tool_use":
                calls.append(block)
    return calls


def detect(detector: dict, evs: list[dict]) -> bool:
    kind = detector["kind"]
    flags = re.IGNORECASE if detector.get("ignore_case") else 0
    pattern = re.compile(detector["pattern"], flags)
    if kind == "regex":
        text = final_text(evs)
        if detector.get("scope") == "first_line":
            text = next((l for l in text.splitlines() if l.strip()), "")
        found = bool(pattern.search(text))
    elif kind == "tool_call":
        found = any(
            c.get("name") == detector["tool"] and pattern.search(json.dumps(c.get("input", {})))
            for c in tool_calls(evs)
        )
    else:
        raise SystemExit(f"policy-replay: unknown detector kind {kind}")
    return found == detector.get("expect", True)


def usage(evs: list[dict]) -> dict:
    model = next((ev.get("model") for ev in evs if ev.get("type") == "system" and ev.get("model")), None)
    for ev in reversed(evs):
        if ev.get("type") == "result":
            u = ev.get("usage", {}) or {}
            return {
                "subtype": ev.get("subtype"),
                "turns": int(ev.get("num_turns", 0) or 0),
                "input_tokens": int(u.get("input_tokens", 0) or 0),
                "output_tokens": int(u.get("output_tokens", 0) or 0),
                "cache_creation_tokens": int(u.get("cache_creation_input_tokens", 0) or 0),
                "cache_read_tokens": int(u.get("cache_read_input_tokens", 0) or 0),
                "model_resolved": model or next(iter(ev.get("modelUsage") or {}), None),
                "cost_api_usd_comparison": round(float(ev.get("total_cost_usd", 0) or 0), 4),
            }
    return {}


def arm_minus(arm: str, scenario: dict) -> list[str]:
    if arm == "full":
        return []
    if arm.startswith("minus:"):
        return [p.strip() for p in arm[len("minus:"):].split(",") if p.strip()]
    if arm.startswith("minus-file:"):
        lines = Path(arm[len("minus-file:"):]).expanduser().read_text().splitlines()
        return [l.strip()[2:].strip() if l.strip().startswith("- ") else l.strip() for l in lines if l.strip()]
    if arm in scenario.get("arms", {}):
        return scenario["arms"][arm]
    raise SystemExit(f"policy-replay: unknown arm {arm!r} for {scenario['id']}")


def run_one(scenario: dict, arm: str, trial: int, policy_text: str) -> dict:
    policy = policy_for_arm(policy_text, arm_minus(arm, scenario))
    model, effort = scenario.get("model", "opus"), scenario.get("effort", "medium")
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="policy-replay-") as work:
        policy_file = Path(work) / "policy.md"
        policy_file.write_text(policy)
        cwd = Path(work) / "cwd"
        cwd.mkdir()
        for rel, content in (scenario.get("setup") or {}).items():
            (cwd / rel).parent.mkdir(parents=True, exist_ok=True)
            (cwd / rel).write_text(content)
        # --setting-sources project in an empty cwd keeps ~/.claude/CLAUDE.md, user hooks and settings out; auth stays as claude -p normally reads it.
        cmd = [
            "claude", "-p", scenario["prompt"],
            "--setting-sources", "project",
            "--append-system-prompt-file", str(policy_file),
            "--model", model,
            "--effort", effort,
            "--max-turns", str(scenario.get("max_turns", 1)),
            "--output-format", "stream-json", "--verbose",
            "--no-session-persistence",
            "--permission-mode", "bypassPermissions",
            "--tools", ",".join(scenario.get("tools", [])),
        ]
        if "max_budget_usd" in scenario:
            cmd += ["--max-budget-usd", str(scenario["max_budget_usd"])]
        env = {k: v for k, v in os.environ.items() if not k.startswith(("AGENT_", "CLAUDE_CODE_SESSION"))}
        try:
            proc = subprocess.run(cmd, cwd=cwd, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=scenario.get("timeout_s", 600))
            stdout, stderr, code = proc.stdout, proc.stderr, proc.returncode
        except subprocess.TimeoutExpired:
            stdout, stderr, code = "", "timeout", "timeout"
    row = {"scenario": scenario["id"], "arm": arm, "trial": trial, "model": model, "effort": effort}
    evs = events(stdout)
    if not any(ev.get("type") == "result" for ev in evs):
        row.update({"pass": None, "error": f"no result (exit {code}): {stderr[-300:]}"})
    else:
        row["pass"] = detect(scenario["detector"], evs)
        row.update(usage(evs))
        if row["subtype"] != "success":
            row.update({"pass": None, "error": f"run ended {row['subtype']} before its final answer"})
        row["tools_called"] = [c.get("name") for c in tool_calls(evs)]
        row["first_line"] = next((l for l in final_text(evs).splitlines() if l.strip()), "")[:200]
    row["wall_s"] = round(time.monotonic() - started, 1)
    return row


def summary(rows: list[dict]) -> str:
    cells: dict[tuple[str, str], list[dict]] = {}
    for r in rows:
        cells.setdefault((r["scenario"], r["arm"]), []).append(r)
    head = ("scenario", "arm", "pass", "err", "tokens in/out/cache", "wall_s")
    table = [head]
    for (scen, arm), rs in cells.items():
        ok = [r for r in rs if r["pass"] is not None]
        passed = sum(1 for r in ok if r["pass"])
        tok = lambda k: sum(r.get(k, 0) for r in ok)
        table.append((
            scen, arm, f"{passed}/{len(ok)}", str(len(rs) - len(ok)),
            f"{tok('input_tokens')}/{tok('output_tokens')}/{tok('cache_creation_tokens') + tok('cache_read_tokens')}",
            f"{sum(r['wall_s'] for r in rs):.0f}",
        ))
    widths = [max(len(row[i]) for row in table) for i in range(len(head))]
    return "\n".join("  ".join(c.ljust(w) for c, w in zip(row, widths)).rstrip() for row in table)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("scenarios", type=Path, nargs="+")
    ap.add_argument("--arm", action="append", required=True,
                    help="full, minus:PREFIX[,PREFIX...], minus-file:PATH (one rule prefix per line) or a scenario arm name; repeatable")
    ap.add_argument("--trials", type=int, default=1)
    ap.add_argument("--jobs", type=int, default=4, help="parallel runs, at most 4")
    ap.add_argument("--policy", type=Path, help="policy text (default: AGENTS.md beside each scenario directory)")
    ap.add_argument("--out", type=Path, required=True, help="results jsonl, appended")
    args = ap.parse_args()

    plan = []
    for path in args.scenarios:
        scenario = json.loads(path.read_text())
        policy_text = (args.policy or path.resolve().parent.parent / "AGENTS.md").read_text()
        for arm in args.arm:
            policy_for_arm(policy_text, arm_minus(arm, scenario))
            plan += [(scenario, arm, t, policy_text) for t in range(1, args.trials + 1)]

    args.out.parent.mkdir(parents=True, exist_ok=True)
    rows = []
    with ThreadPoolExecutor(max_workers=max(1, min(args.jobs, 4))) as pool:
        for fut in as_completed([pool.submit(run_one, *p) for p in plan]):
            row = fut.result()
            with args.out.open("a") as fh:
                fh.write(json.dumps(row) + "\n")
            rows.append(row)
            print(json.dumps(row), file=sys.stderr)
    rows.sort(key=lambda r: (r["scenario"], r["arm"], r["trial"]))
    print(summary(rows))
    return 1 if any(r["pass"] is None for r in rows) else 0


if __name__ == "__main__":
    sys.exit(main())
