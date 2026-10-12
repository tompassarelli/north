#!/usr/bin/env python3
"""Replay one recorded policy scenario through headless `claude -p` and score it."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


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


def usage(evs: list[dict]) -> tuple[int, int, float]:
    for ev in reversed(evs):
        if ev.get("type") == "result":
            u = ev.get("usage", {}) or {}
            tokens = sum(int(u.get(k, 0) or 0) for k in (
                "input_tokens", "output_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"))
            return int(ev.get("num_turns", 0) or 0), tokens, float(ev.get("total_cost_usd", 0) or 0)
    return 0, 0, 0.0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("scenario", type=Path)
    ap.add_argument("--arm", required=True, help="arm name from the scenario's arms")
    ap.add_argument("--trial", type=int, default=1)
    ap.add_argument("--policy", type=Path, help="policy text (default: AGENTS.md beside the scenario directory)")
    ap.add_argument("--out", type=Path, default=Path.home() / ".local/state/agents/replay/results.jsonl")
    args = ap.parse_args()

    scenario = json.loads(args.scenario.read_text())
    policy_path = args.policy or args.scenario.resolve().parent.parent / "AGENTS.md"
    policy = policy_for_arm(policy_path.read_text(), scenario["arms"][args.arm])

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
            "--model", scenario.get("model", "opus"),
            "--effort", scenario.get("effort", "medium"),
            "--max-turns", str(scenario.get("max_turns", 1)),
            "--output-format", "stream-json", "--verbose",
            "--no-session-persistence",
            "--permission-mode", "bypassPermissions",
            "--tools", ",".join(scenario.get("tools", [])),
        ]
        if "max_budget_usd" in scenario:
            cmd += ["--max-budget-usd", str(scenario["max_budget_usd"])]
        env = {k: v for k, v in os.environ.items() if not k.startswith(("AGENT_", "CLAUDE_CODE_SESSION"))}
        proc = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, timeout=scenario.get("timeout_s", 600))

    evs = events(proc.stdout)
    if not any(ev.get("type") == "result" for ev in evs):
        sys.stderr.write(proc.stderr[-2000:])
        raise SystemExit(f"policy-replay: claude -p produced no result (exit {proc.returncode})")
    turns, tokens, cost = usage(evs)
    row = {
        "scenario": scenario["id"], "arm": args.arm, "trial": args.trial,
        "pass": detect(scenario["detector"], evs), "turns": turns, "tokens": tokens,
        "cost_usd": round(cost, 4), "first_line": next((l for l in final_text(evs).splitlines() if l.strip()), "")[:200],
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("a") as fh:
        fh.write(json.dumps(row) + "\n")
    print(json.dumps(row))
    print(f"tokens {tokens} cost ${cost:.4f}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
