#!/usr/bin/env python3
"""Replay policy scenarios x arms x trials through headless `claude -p`, score each run and print pass rates."""

from __future__ import annotations

import argparse
import ctypes
import functools
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
from threading import RLock
import time
import tomllib
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
from policy_stats import operating_point, wilson  # noqa: E402

STATE = Path.home() / ".local/state/agents/replay"
BOX_TOOLS = ["Bash", "Read", "Edit", "Write", "Glob", "Grep"]
PR_SET_CHILD_SUBREAPER = 36
LIVE_GROUPS: dict[subprocess.Popen, dict] = {}
GROUP_LOCK = RLock()
STOPPING = False


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


def start_group(cmd: list[str], **kwargs) -> subprocess.Popen:
    with GROUP_LOCK:
        if STOPPING:
            raise RuntimeError("replay is stopping")
        if ctypes.CDLL(None, use_errno=True).prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0:
            raise OSError(ctypes.get_errno(), "cannot enable child reaping")
        proc = subprocess.Popen(cmd, start_new_session=True, **kwargs)
        LIVE_GROUPS[proc] = {"started": time.monotonic()}
        return proc


def stop_group(proc: subprocess.Popen) -> None:
    with GROUP_LOCK:
        if proc not in LIVE_GROUPS:
            return
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait()
        while True:
            try:
                os.waitpid(-proc.pid, 0)
            except ChildProcessError:
                break
        del LIVE_GROUPS[proc]


def soft_stop(cap, reading, measurement, now=None, grace=300):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
    from usage import hard_stop_due, remaining_p75
    now = time.monotonic() if now is None else now
    with GROUP_LOCK:
        groups = list(LIVE_GROUPS.items())
        remaining = [remaining_p75(measurement, now - entry["started"]) for _, entry in groups]
        if reading < cap + 1 and not any("hard_at" in entry for _, entry in groups) and remaining \
                and all(p is not None for p in remaining) and sum(remaining) <= max(0, cap - reading):
            return []
        events_ = []
        for proc, entry in groups:
            if "hard_at" not in entry:
                entry["hard_at"] = now + grace
                events_.append("graceful")
            if hard_stop_due(entry["hard_at"], now, reading, cap):
                events_.append("hard")
                stop_group(proc)
        return events_


def wait_group(proc: subprocess.Popen, timeout: float) -> int:
    deadline = time.monotonic() + timeout
    while True:
        with GROUP_LOCK:
            if proc not in LIVE_GROUPS:
                return proc.returncode
            if os.waitid(os.P_PID, proc.pid, os.WEXITED | os.WNOWAIT | os.WNOHANG) is not None:
                stop_group(proc)
                return proc.returncode
        if time.monotonic() >= deadline:
            raise subprocess.TimeoutExpired(proc.args, timeout)
        time.sleep(min(0.01, max(0, deadline - time.monotonic())))


def install_signal_handlers() -> None:
    def stopped(signum, frame):
        global STOPPING
        with GROUP_LOCK:
            STOPPING = True
            for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
                signal.signal(sig, signal.SIG_IGN)
            groups = list(LIVE_GROUPS)
            for proc in groups:
                try:
                    os.killpg(proc.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            for proc in groups:
                stop_group(proc)
        raise SystemExit(128 + signum)

    for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(sig, stopped)


def run_check(command: str, cwd: Path, timeout: float) -> bool:
    proc = start_group(["bash", "-c", command], cwd=cwd, stdin=subprocess.DEVNULL,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        return wait_group(proc, timeout) == 0
    except subprocess.TimeoutExpired:
        stop_group(proc)
        return False
    except BaseException:
        stop_group(proc)
        raise


def detect(detector: dict, evs: list[dict], cwd: Path | None = None) -> bool:
    kind = detector["kind"]
    flags = re.IGNORECASE if detector.get("ignore_case") else 0
    pattern = re.compile(detector.get("pattern", ""), flags)
    if kind == "command":
        if cwd is None:
            raise SystemExit("policy-replay: a command detector needs the run's cwd")
        found = run_check(detector["command"], cwd, detector.get("timeout_s", 120))
    elif kind == "regex":
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


def is_box(scenario: dict) -> bool:
    return scenario.get("kind") == "box"


def git(*args: str, cwd: Path | None = None) -> bytes:
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True).stdout


def box_checkout(scenario: dict, work: Path, cwd: Path) -> dict[str, bytes]:
    src = work / "src"
    git("clone", "--quiet", "--no-checkout", scenario["repo"], str(src))
    commit = scenario["commit"]
    hidden = {rel: git("show", f"{commit}:{rel}", cwd=src) for rel in scenario["detector"]["tests"]}
    subprocess.run(["tar", "-x", "-C", str(cwd)], input=git("archive", "--format=tar", f"{commit}^", cwd=src), check=True)
    shutil.rmtree(src)
    git("init", "--quiet", cwd=cwd)
    git("add", "-A", cwd=cwd)
    git("-c", "user.name=policy-replay", "-c", "user.email=policy-replay@localhost", "commit", "--quiet",
        "--allow-empty", "-m", "base", cwd=cwd)
    return hidden


def box_score(scenario: dict, hidden: dict[str, bytes], cwd: Path) -> bool:
    for rel, content in hidden.items():
        (cwd / rel).parent.mkdir(parents=True, exist_ok=True)
        (cwd / rel).write_bytes(content)
    detector = scenario["detector"]
    return run_check(detector["cmd"], cwd, detector.get("timeout_s", 600))


def prepare(scenario: dict, work: Path, cwd: Path) -> dict[str, bytes]:
    if is_box(scenario):
        return box_checkout(scenario, work, cwd)
    for rel, content in (scenario.get("setup") or {}).items():
        (cwd / rel).parent.mkdir(parents=True, exist_ok=True)
        (cwd / rel).write_text(content)
    return {}


def score(scenario: dict, hidden: dict[str, bytes], evs: list[dict], cwd: Path) -> bool:
    return box_score(scenario, hidden, cwd) if is_box(scenario) else detect(scenario["detector"], evs, cwd)


def prompt(scenario: dict) -> str:
    return scenario["brief"] if is_box(scenario) else scenario["prompt"]


def tools(scenario: dict) -> list[str]:
    return scenario.get("tools", BOX_TOOLS if is_box(scenario) else [])


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
                "failure": (ev.get("result") or "result is_error with no text") if ev.get("is_error") else None,
            }
    return {}


def advisor_usage(evs: list[dict]) -> dict:
    calls = {b.get("id") for ev in evs if ev.get("type") == "assistant"
             for b in ev.get("message", {}).get("content", []) or []
             if isinstance(b, dict) and b.get("type") == "server_tool_use" and b.get("name") == "advisor"}
    main = next((ev.get("model") for ev in evs if ev.get("type") == "system" and ev.get("model")), None)
    result = next((ev for ev in reversed(evs) if ev.get("type") == "result"), {})
    named = [i.get("model") for i in (result.get("usage") or {}).get("iterations") or [] if i.get("type") == "advisor_message"]
    others = {m: u for m, u in (result.get("modelUsage") or {}).items() if m != main}
    errors: dict[str, int] = {}
    for ev in (e for e in evs if e.get("type") == "assistant"):
        for b in (ev.get("message") or {}).get("content") or []:
            content = b.get("content") if isinstance(b, dict) and b.get("type") == "advisor_tool_result" else None
            if isinstance(content, dict) and content.get("type") == "advisor_tool_result_error":
                code = str(content.get("error_code"))
                errors[code] = errors.get(code, 0) + 1
    row = {"advisor_calls": len(calls), "advisor_model_resolved": next(iter(named), None) or next(iter(others), None),
           "advisor_errors": errors}
    if others:
        row["advisor_input_tokens"] = sum(int(u.get("inputTokens", 0) or 0) + int(u.get("cacheReadInputTokens", 0) or 0)
                                          + int(u.get("cacheCreationInputTokens", 0) or 0) for u in others.values())
        row["advisor_output_tokens"] = sum(int(u.get("outputTokens", 0) or 0) for u in others.values())
        row["advisor_usd"] = round(sum(float(u.get("costUSD", 0) or 0) for u in others.values()), 4)
    return row


def advisor_debug(text: str) -> dict:
    enabled = re.findall(r"\[AdvisorTool\] Server-side tool enabled with (\S+)", text)
    return {"advisor_attached": bool(enabled), "advisor_model_resolved": enabled[-1] if enabled else None}


def meter(evs: list[dict]) -> dict:
    ev = next((e for e in evs if e.get("type") == "rate_limit_event"), None)
    windows = ((ev or {}).get("rate_limit_info") or {}).get("unifiedWindows") or {}
    pct = lambda w: round(100 * float(windows[w]["utilization"])) if (windows.get(w) or {}).get("utilization") is not None else None
    return {"five_hour_pct": pct("five_hour"), "seven_day_pct": pct("seven_day")}


def no_fable(*names: str | None) -> None:
    if any(n and "fable" in n.lower() for n in names):
        raise SystemExit("policy-replay: Fable is never the main or advisor model here")


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
    firn = os.environ.get("NORTH_FIRN") or str(Path.home() / "code/nixos-config/main")
    out = subprocess.run([os.environ.get("NORTH_BIN", "north"), "agent-config-check", "policy-gate", "measure", "--repo", firn, "--operator", str(operator)],
                         capture_output=True, text=True, timeout=120).stdout
    match = re.search(r"^session start (\d+) tokens", out, re.M)
    if not match:
        raise SystemExit(f"policy-replay: no session start measurement from agent-config-check: {out[-300:]}")
    return int(match.group(1))


def run_tagged(cmd: list[str], cwd: Path, env: dict, timeout: float) -> tuple[str, str, int | str, bytes, subprocess.Popen]:
    tag = f"POLICY_REPLAY_RUN={uuid.uuid4().hex}"
    env = dict(env, POLICY_REPLAY_RUN=tag.partition("=")[2])
    with tempfile.TemporaryFile("w+") as out, tempfile.TemporaryFile("w+") as err:
        proc = start_group(cmd, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=out, stderr=err, text=True)
        try:
            code = wait_group(proc, timeout)
        except subprocess.TimeoutExpired:
            stop_group(proc)
            return "", "timeout", "timeout", tag.encode(), proc
        except BaseException:
            stop_group(proc)
            raise
        out.seek(0)
        err.seek(0)
        return out.read(), err.read(), code, tag.encode(), proc


def tagged(tag: bytes) -> list[int]:
    pids = []
    for proc in Path("/proc").glob("[0-9]*"):
        try:
            if tag in proc.joinpath("environ").read_bytes().split(b"\0") and proc.joinpath("stat").read_text().rpartition(")")[2].split()[0] != "Z":
                pids.append(int(proc.name))
        except (OSError, IndexError):
            continue
    return pids


def reap(tag: bytes, proc: subprocess.Popen) -> int:
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
    stop_group(proc)
    for pid in found:
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass
    return len(found)


def run_one(scenario: dict, arm: str, trial: int, policy_text: str, pick: dict | None = None) -> dict:
    policy = policy_text if arm.startswith("variant:") else policy_for_arm(policy_text, arm_minus(arm, scenario))
    pick = pick or {}
    model, effort = pick.get("model") or scenario.get("model", "opus"), pick.get("effort") or scenario.get("effort", "medium")
    advisor = pick.get("advisor") or scenario.get("advisor")
    no_fable(model, advisor)
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="policy-replay-") as work:
        policy_file = Path(work) / "policy.md"
        policy_file.write_text(policy)
        cwd = Path(work) / "cwd"
        cwd.mkdir()
        hidden = prepare(scenario, Path(work), cwd)
        # --setting-sources project in an empty cwd keeps ~/.claude/CLAUDE.md, user hooks and settings out; auth stays as claude -p normally reads it.
        cmd = [
            "claude", "-p", prompt(scenario),
            "--setting-sources", "project",
            "--append-system-prompt-file", str(policy_file),
            "--model", model,
            "--effort", effort,
            "--max-turns", str(scenario.get("max_turns", 100 if is_box(scenario) else 1)),
            "--output-format", "stream-json", "--verbose",
            "--no-session-persistence",
            "--permission-mode", "bypassPermissions",
            "--tools", ",".join(tools(scenario)),
            "--strict-mcp-config",
            "--debug-file", str(Path(work) / "debug.log"),
        ]
        if "max_budget_usd" in scenario:
            cmd += ["--max-budget-usd", str(scenario["max_budget_usd"])]
        if advisor:
            cmd += ["--advisor", advisor]
        token = Path.home() / ".local/state/north/claude-tokens" / Path(os.environ.get("CLAUDE_CONFIG_DIR", "")).name
        env = {k: v for k, v in os.environ.items() if not k.startswith(("AGENT_", "CLAUDE_CODE_SESSION"))} | (
            {} if os.environ.get("CLAUDE_CODE_OAUTH_TOKEN") or not token.is_file() else {"CLAUDE_CODE_OAUTH_TOKEN": token.read_text().strip()})
        if not advisor:
            env["CLAUDE_CODE_DISABLE_ADVISOR_TOOL"] = "1"
        stdout, stderr, code, tag, proc = run_tagged(cmd, cwd, env, scenario.get("timeout_s", 600))
        row = {"scenario": scenario["id"], "arm": arm, "trial": trial, "model": model, "effort": effort,
               "advisor": advisor, "claude_version": claude_version()}
        evs = events(stdout)
        if not any(ev.get("type") == "result" for ev in evs):
            row.update({"pass": None, "error": f"no result (exit {code}): {stderr[-300:]}"})
        else:
            row["pass"] = score(scenario, hidden, evs, cwd)
            row.update(usage(evs) | advisor_usage(evs) | meter(evs))
            debug = advisor_debug((Path(work) / "debug.log").read_text(errors="replace") if (Path(work) / "debug.log").exists() else "")
            row.update(advisor_attached=debug["advisor_attached"], advisor_model_resolved=debug["advisor_model_resolved"] or row["advisor_model_resolved"])
            row.update(usd=row["cost_api_usd_comparison"], usd_basis="total_cost_usd list price: the ledger rate needs weekly readouts bracketing the run")
            if row["subtype"] != "success" or row["failure"]:
                row.update({"pass": None, "error": row["failure"] if row["subtype"] == "success" else f"run ended {row['subtype']} before its final answer"})
            elif advisor and not row["advisor_attached"]:
                row.update({"pass": None, "error": "advisor not attached: the debug log has no [AdvisorTool] Server-side tool enabled line"})
            elif not advisor and row["advisor_attached"]:
                row.update({"pass": None, "error": "control run had an advisor attached despite CLAUDE_CODE_DISABLE_ADVISOR_TOOL"})
            row["tools_called"] = [c.get("name") for c in tool_calls(evs)]
            row["first_line"] = next((l for l in final_text(evs).splitlines() if l.strip()), "")[:200]
            row["final_text"] = final_text(evs)
        row["leftover_processes"] = reap(tag, proc)
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
        # north worker-ledger exposes no usage-fraction rate, so subscription $ stays null; cost_api_usd_comparison rows hold list price only.
        "usd_or_null": None,
        "wall_s": round(sum(r["wall_s"] for r in runs), 1),
        "model": common([r.get("model_resolved") or r.get("model") for r in runs]),
        "effort": common([r.get("effort") for r in runs]),
        "claude_version": common([r.get("claude_version") for r in runs]),
    }


POINT_KEYS = ("date", "model", "variant", "policy_sha256", "startup_tokens", "conformance", "n", "evidence")


def point_values(row: dict, evidence: Path) -> dict:
    return {"date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "model": row["model"],
            "variant": row["variant"], "policy_sha256": row["policy_sha256"], "startup_tokens": row["startup_tokens"],
            "conformance": row["conformance"], "n": row["n"], "evidence": str(evidence)}


def toml_value(value) -> str:
    return json.dumps(value) if isinstance(value, str) else str(value).lower() if isinstance(value, bool) else str(value)


def write_candidate(path: Path, row: dict, evidence: Path, provisional: bool) -> None:
    values = point_values(row, evidence)
    lines = [f"{k} = {toml_value(values[k])}" for k in POINT_KEYS]
    if provisional:
        lines.append("provisional = true")
    lines.append("")
    lines.append("[scenarios]")
    lines += [f"{json.dumps(s)} = [{p}, {n}]" for s, (p, n) in sorted(row["scenarios"].items())]
    path.parent.mkdir(parents=True, exist_ok=True)
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
    out = args.out or operator / "replay" / f"sweep-{stamp}.jsonl"
    runs = run_plan(plan, args.jobs, STATE / f"{out.stem}-runs.jsonl")
    rows = [variant_row(name, policies[name], startup[name], [r for r in runs if r["arm"] == f"variant:{name}"])
            for name in policies]
    with out.open("a") as fh:
        for row in rows:
            fh.write(json.dumps(row) + "\n")
    print(summary(runs))
    for row in rows:
        print(f"{row['variant']}: startup {row['startup_tokens']} tokens, conformance {row['conformance']} (n={row['n']}), {row['runs_tokens']} tokens/run, {row['wall_s']}s")
    point = operating_point(rows)
    target = args.candidate
    write_candidate(target, point, out, args.provisional)
    lo, hi = wilson(point["conformance"], point["n"])
    print(f"operating point: {point['variant']} ({point['startup_tokens']} tokens, conformance {point['conformance']} "
          f"[{lo:.4f}, {hi:.4f}]) -> {target}")
    return 1 if any(r["pass"] is None for r in runs) else 0


def scenario_policy(path: Path, scenario: dict, override: Path | None) -> Path:
    if override:
        return override
    if scenario.get("policy"):
        return (path.resolve().parent / Path(scenario["policy"]).expanduser()).resolve()
    return path.resolve().parent.parent / "AGENTS.md"


def main() -> int:
    install_signal_handlers()
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("scenarios", type=Path, nargs="+")
    ap.add_argument("--arm", action="append",
                    help="full, minus:PREFIX[,PREFIX...], minus-file:PATH (one rule prefix per line) or a scenario arm name; repeatable")
    ap.add_argument("--sweep", type=Path, help="VARIANTS.toml: run every [[variant]] instead of arms and write the operating-point candidate")
    ap.add_argument("--variant", action="append", help="with --sweep, run only these variant names; repeatable")
    ap.add_argument("--candidate", type=Path, default=STATE / "operating-point-candidate.toml",
                    help="the operating-point candidate --sweep writes")
    ap.add_argument("--provisional", action="store_true", help="with --sweep, mark the operating point provisional (smoke run)")
    ap.add_argument("--trials", type=int, default=1)
    ap.add_argument("--jobs", type=int, default=4, help="parallel runs, at most 4")
    ap.add_argument("--policy", type=Path, help="policy text (default: the scenario's policy field, relative to its file, else AGENTS.md beside its directory)")
    ap.add_argument("--out", type=Path, help="results jsonl, appended (with --sweep default: sweep-<UTC date>.jsonl in the scenario directory, shared through south; its runs stay in the replay state dir)")
    ap.add_argument("--model", help="per-run model overriding the scenario's: sonnet, haiku, opus or a full id")
    ap.add_argument("--effort", choices=["low", "medium", "high", "xhigh"], help="per-run effort overriding the scenario's")
    ap.add_argument("--advisor", help="claude only: advisor model passed as `claude -p --advisor MODEL`")
    args = ap.parse_args()
    pick = {"model": args.model, "effort": args.effort, "advisor": args.advisor}
    no_fable(args.model, args.advisor)
    if args.sweep and any(pick.values()):
        ap.error("--model, --effort and --advisor are per-run arms; --sweep keeps each scenario's model")
    if args.sweep:
        return sweep(args)
    if not args.arm or not args.out:
        ap.error("--arm and --out are required without --sweep")

    plan = []
    for path in args.scenarios:
        scenario = json.loads(path.read_text())
        policy_text = scenario_policy(path, scenario, args.policy).read_text()
        for arm in args.arm:
            policy_for_arm(policy_text, arm_minus(arm, scenario))
            plan += [(scenario, arm, t, policy_text, pick) for t in range(1, args.trials + 1)]

    rows = run_plan(plan, args.jobs, args.out)
    print(summary(rows))
    return 1 if any(r["pass"] is None for r in rows) else 0


if __name__ == "__main__":
    sys.exit(main())
