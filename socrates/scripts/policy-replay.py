#!/usr/bin/env python3
"""Replay policy scenarios x arms x trials through headless `claude -p`, score each run and print pass rates."""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import tomllib

STATE = Path.home() / ".local/state/agents/replay"


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
        return prefixes_from_file(arm[len("minus-file:"):])
    if arm in scenario.get("arms", {}):
        return scenario["arms"][arm]
    raise SystemExit(f"policy-replay: unknown arm {arm!r} for {scenario['id']}")


def prefixes_from_file(path: str) -> list[str]:
    lines = Path(path).expanduser().read_text().splitlines()
    return [l.strip()[2:].strip() if l.strip().startswith("- ") else l.strip() for l in lines if l.strip()]


def strip_frontmatter(text: str) -> str:
    match = re.match(r"---\n.*?\n---\n", text, re.S)
    return text[match.end():] if match else text


def variant_policy(base: str, variant: dict) -> str:
    minus = list(variant.get("minus", []))
    for path in variant.get("minus_file", []):
        minus += prefixes_from_file(path)
    for move in variant.get("placement", []):
        if move.get("to") not in ("skill", "hook"):
            raise SystemExit(f"policy-replay: placement of {move.get('rule')!r} needs to = skill or hook")
        minus.append(move["rule"])
    text = policy_for_arm(base, list(dict.fromkeys(minus)))
    for path in variant.get("plus", []):
        text = text.rstrip("\n") + "\n\n" + strip_frontmatter(Path(path).expanduser().read_text()).strip("\n") + "\n"
    return text


def session_start_tokens(operator: Path) -> int:
    script = Path(__file__).resolve().parent / "agent-config-check.sh"
    firn = os.environ.get("NORTH_FIRN") or str(Path.home() / "code/nixos-config/main")
    env = dict(os.environ, AGENT_CONFIG_PYTHON=os.environ.get("NORTH_AGENT_PYTHON") or sys.executable, OPERATOR=str(operator))
    out = subprocess.run(["bash", "-c", '. "$1"; policy_gate "$2" measure', "-", str(script), firn],
                         env=env, capture_output=True, text=True, timeout=120).stdout
    match = re.search(r"^session start (\d+) tokens", out, re.M)
    if not match:
        raise SystemExit(f"policy-replay: no session start measurement from agent-config-check: {out[-300:]}")
    return int(match.group(1))


def run_one(scenario: dict, arm: str, trial: int, policy_text: str) -> dict:
    policy = policy_text if arm.startswith("variant:") else policy_for_arm(policy_text, arm_minus(arm, scenario))
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


def run_plan(plan: list[tuple], jobs: int, out: Path) -> list[dict]:
    out.parent.mkdir(parents=True, exist_ok=True)
    rows = []
    with ThreadPoolExecutor(max_workers=max(1, min(jobs, 4))) as pool:
        for fut in as_completed([pool.submit(run_one, *p) for p in plan]):
            row = fut.result()
            with out.open("a") as fh:
                fh.write(json.dumps(row) + "\n")
            rows.append(row)
            print(json.dumps(row), file=sys.stderr)
    rows.sort(key=lambda r: (r["scenario"], r["arm"], r["trial"]))
    return rows


def common(values: list) -> str | None:
    values = sorted({v for v in values if v})
    return ",".join(values) or None


def variant_row(name: str, policy: str, startup: int, runs: list[dict]) -> dict:
    scored = [r for r in runs if r["pass"] is not None]
    tokens = [sum(r.get(k, 0) for k in ("input_tokens", "output_tokens", "cache_creation_tokens", "cache_read_tokens")) for r in scored]
    return {
        "variant": name,
        "policy_sha256": hashlib.sha256(policy.encode()).hexdigest(),
        "startup_tokens": startup,
        "conformance": round(sum(1 for r in scored if r["pass"]) / len(scored), 4) if scored else None,
        "n": len(scored),
        "errors": len(runs) - len(scored),
        "runs_tokens": round(sum(tokens) / len(tokens)) if tokens else None,
        # worker-ledger exposes no usage-fraction rate, so subscription $ stays null; cost_api_usd_comparison rows hold list price only.
        "usd_or_null": None,
        "wall_s": round(sum(r["wall_s"] for r in runs), 1),
        "model": common([r.get("model_resolved") or r.get("model") for r in runs]),
        "effort": common([r.get("effort") for r in runs]),
    }


def operating_point(rows: list[dict]) -> dict:
    scored = [r for r in rows if r["conformance"] is not None and r["n"]]
    if not scored:
        raise SystemExit("policy-replay: no variant produced a scored run")
    best = max(scored, key=lambda r: r["conformance"])
    se = math.sqrt(best["conformance"] * (1 - best["conformance"]) / best["n"])
    return min((r for r in scored if r["conformance"] >= best["conformance"] - se - 1e-9),
               key=lambda r: (r["startup_tokens"], -r["conformance"]))


def write_operating_point(path: Path, row: dict, evidence: Path, provisional: bool) -> None:
    lines = [
        f'date = "{datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")}"',
        f'model = "{row["model"]}"',
        f'variant = "{row["variant"]}"',
        f'policy_sha256 = "{row["policy_sha256"]}"',
        f'startup_tokens = {row["startup_tokens"]}',
        f'conformance = {row["conformance"]}',
        f'n = {row["n"]}',
        f'evidence = "{evidence}"',
    ]
    if provisional:
        lines.append("provisional = true")
    path.write_text("\n".join(lines) + "\n")


def sweep(args: argparse.Namespace) -> int:
    variants = tomllib.loads(args.sweep.read_text())["variant"]
    if args.variant:
        unknown = set(args.variant) - {v["name"] for v in variants}
        if unknown:
            raise SystemExit(f"policy-replay: unknown variants {sorted(unknown)}")
        variants = [v for v in variants if v["name"] in args.variant]
    operator = args.scenarios[0].resolve().parent.parent
    base = (args.policy or operator / "AGENTS.md").read_text()
    measured = session_start_tokens(operator)
    policies = {v["name"]: variant_policy(base, v) for v in variants}
    startup = {name: measured + (len(text) - len(base)) // 4 for name, text in policies.items()}
    scenarios = [json.loads(p.read_text()) for p in args.scenarios]
    plan = [(s, f"variant:{name}", t, text) for name, text in policies.items()
            for s in scenarios for t in range(1, args.trials + 1)]
    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%d")
    out = args.out or STATE / f"sweep-{stamp}.jsonl"
    runs = run_plan(plan, args.jobs, out.with_name(out.stem + "-runs.jsonl"))
    rows = [variant_row(name, policies[name], startup[name], [r for r in runs if r["arm"] == f"variant:{name}"])
            for name in policies]
    with out.open("a") as fh:
        for row in rows:
            fh.write(json.dumps(row) + "\n")
    print(summary(runs))
    for row in rows:
        print(f"{row['variant']}: startup {row['startup_tokens']} tokens, conformance {row['conformance']} (n={row['n']}), {row['runs_tokens']} tokens/run, {row['wall_s']}s")
    point = operating_point(rows)
    target = args.operating_point or operator / "operating-point.toml"
    write_operating_point(target, point, out, args.provisional)
    print(f"operating point: {point['variant']} ({point['startup_tokens']} tokens, conformance {point['conformance']}) -> {target}")
    return 1 if any(r["pass"] is None for r in runs) else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("scenarios", type=Path, nargs="+")
    ap.add_argument("--arm", action="append",
                    help="full, minus:PREFIX[,PREFIX...], minus-file:PATH (one rule prefix per line) or a scenario arm name; repeatable")
    ap.add_argument("--sweep", type=Path, help="VARIANTS.toml: run every [[variant]] instead of arms and write the operating point")
    ap.add_argument("--variant", action="append", help="with --sweep, run only these variant names; repeatable")
    ap.add_argument("--operating-point", type=Path, help="with --sweep (default: operating-point.toml beside the scenario directory)")
    ap.add_argument("--provisional", action="store_true", help="with --sweep, mark the operating point provisional (smoke run)")
    ap.add_argument("--trials", type=int, default=1)
    ap.add_argument("--jobs", type=int, default=4, help="parallel runs, at most 4")
    ap.add_argument("--policy", type=Path, help="policy text (default: AGENTS.md beside each scenario directory)")
    ap.add_argument("--out", type=Path, help="results jsonl, appended (with --sweep default: sweep-<UTC date>.jsonl in the replay state dir)")
    args = ap.parse_args()
    if args.sweep:
        return sweep(args)
    if not args.arm or not args.out:
        ap.error("--arm and --out are required without --sweep")

    plan = []
    for path in args.scenarios:
        scenario = json.loads(path.read_text())
        policy_text = (args.policy or path.resolve().parent.parent / "AGENTS.md").read_text()
        for arm in args.arm:
            policy_for_arm(policy_text, arm_minus(arm, scenario))
            plan += [(scenario, arm, t, policy_text) for t in range(1, args.trials + 1)]

    rows = run_plan(plan, args.jobs, args.out)
    print(summary(rows))
    return 1 if any(r["pass"] is None for r in rows) else 0


if __name__ == "__main__":
    sys.exit(main())
