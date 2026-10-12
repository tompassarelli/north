#!/usr/bin/env python3
"""Replay policy scenarios x arms x trials through headless `claude -p` or `codex exec`, score each run and print pass rates."""

from __future__ import annotations

import argparse
import difflib
import functools
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time
import tomllib
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
from policy_stats import operating_point, wilson  # noqa: E402

STATE = Path.home() / ".local/state/agents/replay"
CODEX_HOME = Path(os.environ.get("NORTH_CODEX_HOME") or Path.home() / ".local/state/north/codex-pooled")
CODEX_MODEL, CODEX_EFFORT = "gpt-6.1-sol", "medium"
CAPACITY = re.compile(r"at capacity|server_overloaded", re.IGNORECASE)


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


def fenced_blocks(text: str) -> list[tuple[str, list[str]]]:
    blocks, prose, body = [], "", None
    for line in text.splitlines():
        if line.lstrip().startswith("```"):
            if body is None:
                body = []
            else:
                blocks.append((prose, body))
                body, prose = None, ""
        elif body is not None:
            body.append(line)
        elif line.strip():
            prose = line
    return blocks


def offered_command(pattern: re.Pattern, warning: re.Pattern | None, text: str) -> bool:
    for prose, body in fenced_blocks(text):
        comments = [l for l in body if l.strip().startswith("#")]
        warned = warning is not None and any(warning.search(s) for s in [prose, *comments])
        for line in body:
            command, _, trailing = line.partition(" #")
            if line.strip().startswith("#") or not pattern.search(command):
                continue
            if not warned and not (warning is not None and warning.search(trailing)):
                return True
    return False


def detect(detector: dict, evs: list[dict]) -> bool:
    kind = detector["kind"]
    flags = re.IGNORECASE if detector.get("ignore_case") else 0
    pattern = re.compile(detector["pattern"], flags)
    if kind == "regex":
        text = final_text(evs)
        if detector.get("scope") == "first_line":
            text = next((l for l in text.splitlines() if l.strip()), "")
        found = bool(pattern.search(text))
    elif kind == "code_command":
        warning = re.compile(detector["warning"], re.IGNORECASE) if detector.get("warning") else None
        found = offered_command(pattern, warning, final_text(evs))
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
    if variant.get("policy"):
        base = Path(variant["policy"]).expanduser().read_text()
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


def run_tagged(cmd: list[str], cwd: Path, env: dict, timeout: float) -> tuple[str, str, int | str, bytes]:
    tag = f"POLICY_REPLAY_RUN={uuid.uuid4().hex}"
    env = dict(env, POLICY_REPLAY_RUN=tag.partition("=")[2])
    with tempfile.TemporaryFile("w+") as out, tempfile.TemporaryFile("w+") as err:
        proc = subprocess.Popen(cmd, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=out, stderr=err, text=True)
        try:
            code = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            return "", "timeout", "timeout", tag.encode()
        out.seek(0)
        err.seek(0)
        return out.read(), err.read(), code, tag.encode()


def tagged(tag: bytes) -> list[int]:
    pids = []
    for proc in Path("/proc").glob("[0-9]*"):
        try:
            if tag in proc.joinpath("environ").read_bytes().split(b"\0") and proc.joinpath("stat").read_text().rpartition(")")[2].split()[0] != "Z":
                pids.append(int(proc.name))
        except (OSError, IndexError):
            continue
    return pids


def reap(tag: bytes) -> int:
    found = tagged(tag)
    for sig, grace in ((signal.SIGTERM, 3.0), (signal.SIGKILL, 1.0)):
        for pid in tagged(tag):
            try:
                os.kill(pid, sig)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + grace
        while tagged(tag) and time.monotonic() < deadline:
            time.sleep(0.1)
    return len(found)


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
        stdout, stderr, code, tag = run_tagged(cmd, cwd, env, scenario.get("timeout_s", 600))
        row = {"scenario": scenario["id"], "arm": arm, "trial": trial, "model": model, "effort": effort,
               "claude_version": claude_version()}
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
            row["final_text"] = final_text(evs)
        row["leftover_processes"] = reap(tag)
    row["wall_s"] = round(time.monotonic() - started, 1)
    return row


def codex_live_policy(policy_text: str) -> None:
    live = re.sub(r"\A<!--.*?-->\n", "", (CODEX_HOME / "AGENTS.md").read_text())
    if live.strip() != policy_text.strip():
        raise SystemExit(f"policy-replay: --provider codex runs the live policy from {CODEX_HOME}/AGENTS.md; --policy differs from it")


def snapshot(root: Path) -> dict[str, str]:
    files = {}
    for path in root.rglob("*"):
        if path.is_file() and "__pycache__" not in path.parts:
            try:
                files[str(path.relative_to(root))] = path.read_text()
            except UnicodeDecodeError:
                pass
    return files


def diff_edits(root: Path, before: dict[str, str], after: dict[str, str]) -> list[dict]:
    edits = []
    for rel in sorted(set(before) | set(after)):
        old, new = before.get(rel, ""), after.get(rel, "")
        if old == new:
            continue
        a, b = old.splitlines(keepends=True), new.splitlines(keepends=True)
        for tag, i1, i2, j1, j2 in difflib.SequenceMatcher(None, a, b, autojunk=False).get_opcodes():
            if tag != "equal":
                edits.append({"type": "tool_use", "name": "Edit", "input": {
                    "file_path": str(root / rel), "old_string": "".join(a[i1:i2]), "new_string": "".join(b[j1:j2])}})
    return edits


def codex_events(stdout: str, edits: list[dict], model: str) -> list[dict]:
    raw = events(stdout)
    blocks, text, usage_, done = [], "", {}, False
    for ev in raw:
        item = ev.get("item") or {}
        if ev.get("type") == "item.completed" and item.get("type") == "command_execution":
            blocks.append({"type": "tool_use", "name": "Bash", "input": {"command": item.get("command", "")}})
        elif ev.get("type") == "item.completed" and item.get("type") == "agent_message":
            text = item.get("text", "")
        elif ev.get("type") == "turn.completed":
            done, u = True, ev.get("usage") or {}
            usage_ = {"input_tokens": int(u.get("input_tokens", 0) or 0) - int(u.get("cached_input_tokens", 0) or 0),
                      "output_tokens": u.get("output_tokens", 0),
                      "cache_read_input_tokens": u.get("cached_input_tokens", 0),
                      "cache_creation_input_tokens": u.get("cache_write_input_tokens", 0)}
    if not done:
        return []
    return [{"type": "system", "model": model},
            {"type": "assistant", "message": {"content": blocks + edits}},
            {"type": "result", "subtype": "success", "result": text, "num_turns": 1, "usage": usage_}]


def run_codex(scenario: dict, arm: str, trial: int, version: str) -> dict:
    started = time.monotonic()
    outages, code, stdout, stderr = 0, None, "", ""
    with tempfile.TemporaryDirectory(prefix="policy-replay-") as work:
        cwd = Path(work) / "cwd"
        cwd.mkdir()
        for rel, content in (scenario.get("setup") or {}).items():
            (cwd / rel).parent.mkdir(parents=True, exist_ok=True)
            (cwd / rel).write_text(content)
        before = snapshot(cwd)
        # Codex cannot run without tools, so a tool-less scenario runs in a read-only sandbox.
        sandbox = "workspace-write" if scenario.get("tools") else "read-only"
        cmd = ["codex", "exec", "--json", "--ephemeral", "--skip-git-repo-check", "-s", sandbox,
               "-c", "sandbox_workspace_write.network_access=true",
               "-m", CODEX_MODEL, "-c", f"model_reasoning_effort={CODEX_EFFORT}", "-C", str(cwd), scenario["prompt"]]
        env = {k: v for k, v in os.environ.items() if not k.startswith(("AGENT_", "CLAUDE_CODE_SESSION"))}
        env.update(CODEX_HOME=str(CODEX_HOME), RUST_LOG="codex_core=debug")
        for delay in (60, 120, 240, None):
            stdout, stderr, code, tag = run_tagged(cmd, cwd, env, scenario.get("timeout_s", 600))
            failure = stdout + "".join(l for l in stderr.splitlines(keepends=True) if " DEBUG " not in l)
            if not CAPACITY.search(failure) or delay is None:
                break
            reap(tag)
            outages += 1
            time.sleep(delay)
        edits = diff_edits(cwd, before, snapshot(cwd))
        row = {"scenario": scenario["id"], "arm": arm, "trial": trial, "provider": "codex", "codex_version": version,
               "model": CODEX_MODEL, "effort": CODEX_EFFORT, "sandbox": sandbox, "outages": outages}
        resolved = re.findall(r"turn\.id=\S+ model=(\S+)", stderr)
        evs = codex_events(stdout, edits, resolved[-1] if resolved else None)
        if not evs:
            kind = "outage" if CAPACITY.search(failure) else "no result"
            row.update({"pass": None, "error": f"{kind} (exit {code}): {failure[-300:]}"})
        else:
            row["pass"] = detect(scenario["detector"], evs)
            row.update(usage(evs))
            row["tools_called"] = [c.get("name") for c in tool_calls(evs)]
            row["first_line"] = next((l for l in final_text(evs).splitlines() if l.strip()), "")[:200]
            row["final_text"] = final_text(evs)
        row["leftover_processes"] = reap(tag)
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


def run_plan(plan: list[tuple], jobs: int, out: Path, runner=run_one) -> list[dict]:
    out.parent.mkdir(parents=True, exist_ok=True)
    rows = []
    with ThreadPoolExecutor(max_workers=max(1, min(jobs, 4))) as pool:
        for fut in as_completed([pool.submit(runner, *p) for p in plan]):
            row = fut.result()
            with out.open("a") as fh:
                fh.write(json.dumps(row) + "\n")
            rows.append(row)
            print(json.dumps(row), file=sys.stderr)
    rows.sort(key=lambda r: (r["scenario"], r["arm"], r["trial"]))
    return rows


@functools.cache
def claude_version() -> str | None:
    try:
        return subprocess.run(["claude", "--version"], capture_output=True, text=True, timeout=30).stdout.strip() or None
    except (OSError, subprocess.TimeoutExpired):
        return None


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
        "scenarios": {s: [sum(1 for r in scored if r["scenario"] == s and r["pass"]), sum(1 for r in scored if r["scenario"] == s)]
                      for s in sorted({r["scenario"] for r in scored})},
        "errors": len(runs) - len(scored),
        "runs_tokens": round(sum(tokens) / len(tokens)) if tokens else None,
        # worker-ledger exposes no usage-fraction rate, so subscription $ stays null; cost_api_usd_comparison rows hold list price only.
        "usd_or_null": None,
        "wall_s": round(sum(r["wall_s"] for r in runs), 1),
        "model": common([r.get("model_resolved") or r.get("model") for r in runs]),
        "effort": common([r.get("effort") for r in runs]),
        "claude_version": common([r.get("claude_version") for r in runs]),
    }


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
    lo, hi = wilson(point["conformance"], point["n"])
    print(f"operating point: {point['variant']} ({point['startup_tokens']} tokens, conformance {point['conformance']} "
          f"[{lo:.4f}, {hi:.4f}]) -> {target}")
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
    ap.add_argument("--provider", choices=["claude", "codex"], default="claude",
                    help="codex runs `codex exec` (gpt-6.1-sol medium) from its normal home, so the live policy and skill listing load; only --arm full")
    ap.add_argument("--out", type=Path, help="results jsonl, appended (with --sweep default: sweep-<UTC date>.jsonl in the replay state dir)")
    args = ap.parse_args()
    if args.sweep:
        return sweep(args)
    if not args.arm or not args.out:
        ap.error("--arm and --out are required without --sweep")
    if args.provider == "codex":
        if args.arm != ["full"]:
            ap.error("--provider codex runs only --arm full: the Codex home loads the live policy")
        version = subprocess.run(["codex", "--version"], capture_output=True, text=True).stdout.strip()
        plan = []
        for path in args.scenarios:
            codex_live_policy((args.policy or path.resolve().parent.parent / "AGENTS.md").read_text())
            plan += [(json.loads(path.read_text()), "full", t, version) for t in range(1, args.trials + 1)]
        rows = run_plan(plan, args.jobs, args.out, run_codex)
        print(summary(rows))
        scored = [r for r in rows if r["pass"] is not None]
        if scored:
            rate = sum(1 for r in scored if r["pass"]) / len(scored)
            lo, hi = wilson(rate, len(scored))
            print(f"codex conformance {rate:.4f} [{lo:.4f}, {hi:.4f}] n={len(scored)}, outages {sum(r['outages'] for r in rows)}")
        return 1 if any(r["pass"] is None for r in rows) else 0

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
