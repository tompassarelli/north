#!/usr/bin/env python3
"""Spawn gate: refuse a new local worker once provisioned CPU is used up.

Shared by the Claude PreToolUse(Agent) hook (spawn-capacity-guard.sh) and the
Codex behavior guard's PreToolUse(spawn_agent). A worker idles while its model
thinks and its heavy commands run inside capacity leases, so admission counts
the CPUs batch work commits (machine-capacity `probe`: committedBatchCpus, each
live batch lease at max(two-minute measured use, estimate p50) plus 25% plus unleased heavy load, against
aggregateCpuLimit; native clients count only through the protected-pressure
check, since their 2-CPU charge per desktop and client overstates them) rather than system CPU pressure. It also refuses while the
protected desktop slice is under pressure (protectedCpuSomeAvg10 above 20; skipped in
the unattended profile, as the helper does). A
missing, slow (2 s) or unreadable helper allows. An urgent fix passes with
`CASE=URGENT FACT="..."` at the start of the brief, logged beside the Codex
behavior overrides. The capacity watchdog (a brief carrying
`[routine:watchdog]`) is never refused, since it reports the overload. A
landing-path worker (a brief line `Landing-path: yes`, or the agent name
`lander`) is admitted at any CPU or pressure level, since the landing path is
the constraint; it holds an agent lease, which charges memory only.

Fleet sessions carry their parent link and task reference. A task reference in a
project below the first in orchestration.toml's [projects] priority order stops at
(1 - PRIORITY_HEADROOM) of the CPU limit.
"""

import json
import os
import re
import subprocess, sys
import time
import tomllib
from pathlib import Path

PROTECTED_PRESSURE_LIMIT = 20.0
HELPER_TIMEOUT = 2.0
LOCAL_CLAUDE_TYPES = {"worker", "worker-high", "worker-xhigh", "worker-haiku", "worker-sonnet", "general-purpose", "Explore", "fork"}
ACTIVE_SECONDS = 300
EXEMPT_MARKER = "[routine:watchdog]"
REGISTRY = next(p for p in (Path(__file__).resolve().parent / "defaults.toml",
                             Path(__file__).resolve().parents[2] / "defaults.toml") if p.exists())
DEFAULTS = {name: entry["value"] for name, entry in tomllib.loads(REGISTRY.read_text()).items()}
PRIORITY_HEADROOM = DEFAULTS["priority_headroom"]
LANDING_PATH = re.compile(r"(?mi)^\W*Landing-path:\s*yes\b")
CLOSING = re.compile(r"(?mi)^\W*Closing:\s*yes\b")
LANDER = re.compile(r"(?i)\blander\b")
NOT_BLOCKED = "warning only, nothing blocked: "
URGENT = re.compile(r"(?m)^\W*CASE=URGENT\s+FACT=(?:[\"“”]([^\"“”]*)[\"“”]|['‘’]([^'‘’]*)['‘’])", re.IGNORECASE)


def helper_path():
    here = Path(__file__).resolve().parent
    for candidate in (
        here.parent.parent / "skills/machine-capacity/scripts/machine-capacity.mjs",
        Path.home() / ".agents/skills/machine-capacity/scripts/machine-capacity.mjs",
    ):
        if candidate.is_file():
            return candidate
    return None


def capacity_probe(command=("probe", "--class", "agent")):
    """The helper's probe JSON as a dict, or None when it can't be read."""
    fixture = os.environ.get("SPAWN_CAPACITY_STATUS")
    try:
        if fixture is not None:
            raw = Path(fixture).read_text()
        else:
            import shutil
            import subprocess
            helper = helper_path()
            bun = shutil.which("bun") or shutil.which("bun", path=str(Path.home() / ".nix-profile/bin"))
            if helper is None or bun is None:
                return None
            raw = subprocess.run([bun, str(helper), *command], capture_output=True,
                                 text=True, timeout=HELPER_TIMEOUT, stdin=subprocess.DEVNULL).stdout
        data = json.loads(raw)
    except Exception:  # a missing, slow or broken helper allows
        return None
    return data if isinstance(data, dict) else None


def number(value):
    return float(value) if isinstance(value, (int, float)) and not isinstance(value, bool) else None


def capacity():
    """(provisioned CPUs, CPU limit, protected pressure) or None to allow."""
    data = capacity_probe()
    if data is None:
        return None
    provisioned = number(data.get("committedBatchCpus"))
    limit = number(data.get("aggregateCpuLimit")) or float(os.cpu_count() or 1)
    protected = number(data.get("protectedCpuSomeAvg10"))
    if provisioned is None or protected is None:
        return None
    if data.get("profile") == "unattended":
        protected = 0.0
    return provisioned, limit, protected


def local_workers(provider=None):
    """Claude subagent transcripts and Codex rollouts written in the last five minutes."""
    home = Path(os.environ.get("SPAWN_CAPACITY_HOME", Path.home()))
    cutoff = time.time() - ACTIVE_SECONDS
    count = 0
    patterns = [
        (home / ".claude" / "projects", "*/*/subagents/*.jsonl"),
        (home / ".codex" / "sessions", time.strftime("%Y/%m/%d") + "/*.jsonl"),
    ]
    if provider is not None:
        patterns = [patterns[0 if provider == "claude" else 1]]
    for root, pattern in patterns:
        try:
            for path in root.glob(pattern):
                try:
                    if path.stat().st_mtime >= cutoff:
                        count += 1
                except OSError:
                    pass
        except OSError:
            pass
    return count


def state_root():
    return Path(
        os.environ.get(
            "CODEX_BEHAVIOR_STATE",
            Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp")) / "codex-behavior",
        )
    )


def log_override(event, fact):
    try:
        root = state_root()
        root.mkdir(parents=True, exist_ok=True)
        with open(root / "verify-overrides.jsonl", "a") as log:
            log.write(json.dumps({
                "at": time.time(), "session": event.get("session_id"), "agent": event.get("agent_id"),
                "gate": "spawn-capacity", "case": "URGENT", "cwd": event.get("cwd"), "fact": fact[:300],
            }) + "\n")
    except OSError:
        pass


def is_local(event):
    tool = str(event.get("tool_name") or "")
    tool_input = event.get("tool_input") or {}
    if tool.endswith("spawn_agent"):
        return True
    if tool != "Agent" or not isinstance(tool_input, dict):
        return False
    if tool_input.get("isolation") == "remote":
        return False
    return (tool_input.get("subagent_type") or "general-purpose") in LOCAL_CLAUDE_TYPES


def brief_texts(tool_input):
    """Every string in the spawn input; Codex may send it as serialized JSON."""
    if isinstance(tool_input, str):
        try:
            parsed = json.loads(tool_input)
        except ValueError:
            return [tool_input]
        return [tool_input] if isinstance(parsed, str) else brief_texts(parsed)
    if isinstance(tool_input, dict):
        return [t for value in tool_input.values() for t in brief_texts(value)]
    if isinstance(tool_input, list):
        return [t for value in tool_input for t in brief_texts(value)]
    return []


def log_denial(event, gate, reason=None):
    try:
        path = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state")) / "agents/spawn-gate-denials.jsonl"
        path.parent.mkdir(parents=True, exist_ok=True)
        texts = brief_texts(event.get("tool_input") or {})
        with path.open("a") as f:
            f.write(json.dumps({"at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "gate": gate, "tool": event.get("tool_name"), "input_type": type(event.get("tool_input")).__name__,
                                "heads": [t[:160] for t in texts[:4]], "reason": reason}) + "\n")
    except OSError:
        pass


def urgent_file_fact(texts):
    """Codex encrypts spawn briefs, so its override names the agent in
    spawn-urgent.tsv (epoch<TAB>agent-name<TAB>fact), written within 15 minutes."""
    path = Path(os.environ.get("SPAWN_CAPACITY_HOME", Path.home())) / ".local/state/agents/spawn-urgent.tsv"
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return None
    names = {t.strip() for t in texts if len(t) <= 80}
    now = time.time()
    for line in reversed(lines):
        parts = line.split("\t", 2)
        if len(parts) == 3 and parts[0].isdigit() and now - int(parts[0]) <= 900 \
                and parts[1] in names and len(parts[2].strip()) >= 15:
            return parts[2].strip()
    return None


def operator_dir():
    configured = os.environ.get("AGENTS_OPERATOR_DIR")
    if configured:
        return Path(configured)
    return Path.home() / "code/south/main/agents"


def orchestration_path():
    return os.environ.get("AGENTS_ORCHESTRATION") or operator_dir() / "orchestration.toml"


def fleet_domain_rank(event=None):
    """(domain, rank in the priority order, first domain) for this Fleet task, or None."""
    domain = fleet_domain(event)
    if domain is None:
        return None
    try:
        import tomllib
        with open(orchestration_path(), "rb") as f:
            projects = tomllib.load(f).get("projects", {})
        order = [d.lower() for d in sorted(projects, key=lambda d: projects[d].get("priority", float("inf")))]
    except (OSError, ValueError, KeyError, TypeError, StopIteration, AttributeError):
        return None
    if domain not in order:
        return None
    return domain, order.index(domain), order[0]


def usage_verdict(event, grant=None):
    """This provider's usage-gate.json verdict (firn#12): a fresh one answers at once, a stale or missing one runs
    `agents usage --gate` (exit 3 carries its refusal) and rereads the file; no command allows."""
    import shutil
    import subprocess
    provider = "claude" if event.get("tool_name") == "Agent" else "codex"
    home = os.environ.get("SPAWN_CAPACITY_HOME")
    state = Path(home) / ".local/state" if home else Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")

    def read():
        try:
            verdict = json.loads((state / "agents/usage-gate.json").read_text())[provider]
            verdict["refusal"] = (verdict.get("refusals") or {}).get(billed_account(provider), verdict.get("refusal"))
            if grant and verdict.get("refusal") is None:
                return verdict
            if not 0 <= time.time() - verdict["epoch"] < min(verdict["stale_min"] * 60, 120):
                return None
            if verdict.get("pace_control"):
                account = billed_account(provider) or str((verdict.get("pace") or {}).get("account") or provider)
                control = (verdict.get("controls") or {}).get(account) or (verdict.get("controls") or {}).get(provider)
                if control is None or not pace_call("fresh", control=control, at=time.time()):
                    return None
            return verdict
        except (OSError, ValueError, KeyError, TypeError):
            return None

    verdict = read()
    if verdict is not None:
        return verdict
    agents = shutil.which("agents") or os.path.expanduser("~/.local/share/north/bin/agents")
    try:
        result = subprocess.run([agents, "usage", "--gate", provider], capture_output=True, text=True, timeout=45)
    except (OSError, subprocess.TimeoutExpired):
        return {"refusal": f"Hold: {provider} usage refresh unavailable; needs a reading <=120s old."}
    verdict = read() or {"refusal": f"Hold: {provider} usage remains stale after refresh; needs a reading <=120s old."}
    if result.returncode == 3 and result.stdout.strip():
        verdict = dict(verdict, refusal=result.stdout.strip())
    return verdict


def fleet_session(event):
    session = (event or {}).get("session_id")
    if not session:
        return None
    try:
        records = json.loads(subprocess.run(
            [os.environ.get("NORTH_BIN", "north"), "fleet", "list", "--json"],
            capture_output=True, text=True, timeout=2, check=True).stdout)
        return next((record for record in records if record.get("conversation") == session), None)
    except (OSError, ValueError, subprocess.SubprocessError, AttributeError, StopIteration):
        return None


def fleet_domain(event=None):
    record = fleet_session(event)
    task_ref = record.get("task_ref") if record else None
    return task_ref.split("#", 1)[0].split("/")[-1].lower() if isinstance(task_ref, str) else None


def north_lib():
    import shutil
    agents = shutil.which("agents") or str(Path.home() / ".local/share/north/bin/agents")
    libraries = [Path(__file__).resolve().parents[2] / "lib", Path(agents).resolve().parent.parent / "harness/lib"]
    library = next(path for path in libraries if (path / "usage.py").is_file())
    if str(library) not in sys.path:
        sys.path.insert(0, str(library))
    return library


def billed_account(provider):
    """The account the signed-in session bills, or None."""
    try:
        north_lib()
        import usage
        label = usage.local_account(provider)
    except (ImportError, OSError, ValueError, StopIteration):
        return None
    return f"{provider}:{label}" if label else None


def pace_call(op, **request):
    result = subprocess.run([os.environ.get("NORTH_PACE_BIN", "north"), "pace"], input=json.dumps(dict(op=op, **request)), capture_output=True, text=True, timeout=10)
    if result.returncode: raise ValueError(result.stderr.strip())
    return json.loads(result.stdout)


def confirm_spawn(event, *, launched=True):
    north_lib()
    home = os.environ.get("SPAWN_CAPACITY_HOME")
    state = Path(home) / ".local/state" if home else Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
    pace_call("confirm", directory=str(state / "agents"), session=event.get("session_id"), tool_use_id=event.get("tool_use_id"), launched=launched)


def spawn_reservation(event, verdict, provider):
    try:
        north_lib()
        import usage
        account = billed_account(provider)
        pace = (verdict.get("paces") or {}).get(account) or verdict.get("pace") or {}
        label = str(account or pace.get("account") or provider).partition(":")[2]
        data = event.get("tool_input") or {}
        if isinstance(data, str):
            data = json.loads(data)
        if not isinstance(data, dict):
            data = {}
        measured = usage.ledger_reservation(provider, data.get("model"), data.get("reasoning_effort") or data.get("effort"),
                                            label=label, details=True) or {}
        expected, existing = measured.get("expected"), measured.get("existing")
        in_flight = local_workers(provider)
        if expected is None:
            return None, in_flight, existing, "no ledger-measured points per run"
        if in_flight and existing is None:
            return expected, in_flight, None, "no ledger-measured points per in-flight run"
        return expected, in_flight, existing if existing is not None else expected, None
    except (ImportError, OSError, ValueError, RuntimeError, StopIteration) as error:
        return None, 0, None, str(error).removeprefix("harness-tuner: cap reached: ")


def account_reservation(event, verdict, provider, expected, in_flight, existing):
    north_lib()
    from usage import reservation_stop
    name = billed_account(provider) or str((verdict.get("pace") or {}).get("account") or provider)
    row = (verdict.get("accounts") or {}).get(name) or (verdict.get("accounts") or {}).get(provider) or {}
    used, cap = number(row.get("used_pct")), number(row.get("cap_pct"))
    if used is not None and cap is not None and used < cap and reservation_stop(used, cap, expected, in_flight, existing):
        return (f"Hold: {name} used {used:g}% + reserve {existing * in_flight:g} points "
                f"({in_flight} in flight) + next {expected:g} points > cap {cap:g}%.")
    return None


def flow_gate(event, texts):
    tool = str(event.get("tool_name") or "")
    tool_input = event.get("tool_input") or {}
    if tool == "Agent" and isinstance(tool_input, dict) and \
            (tool_input.get("subagent_type") or "general-purpose") not in LOCAL_CLAUDE_TYPES:
        return None
    if any(re.search(r"\bCASE=URGENT\b", text) for text in texts) or urgent_file_fact(texts):
        return None
    if any(re.search(r"\b(?:Autoland|safe-push|landing[ -]queue|main[ -]red)\b", text, re.I) for text in texts):
        return None
    if any(LANDING_PATH.search(text) or CLOSING.search(text) for text in texts) or \
            (isinstance(tool_input, dict) and LANDER.search(str(tool_input.get("name") or ""))):
        return None
    repo = None
    if any(re.search(r"(?mi)^\W*Category:\s*research\b", text) for text in texts):
        return None
    for pattern, sources in ((r"code/([A-Za-z0-9][A-Za-z0-9_.-]*)/worktrees/", texts),
                             (r"code/([A-Za-z0-9][A-Za-z0-9_.-]*)/worktrees/", [str(event.get("cwd") or "")])):
        match = next((m for m in (re.search(pattern, text.rstrip("/") + "/") for text in sources) if m), None)
        if match:
            repo = match.group(1)
            break
    if repo is None:
        return None
    try:
        north_lib()
        import flow
        row = flow.get(repo, fields=("pending", "landed_per_hour"))
        pending, rate = number(row["pending"]), number(row["landed_per_hour"])
        if pending is None or rate is None or pending <= max(2, rate):
            return None
        reason = f"{flow.format_row(row)}. Refusing a new code worker: widen the landing path first. A closing pass that pushes no lane passes with a `Closing: yes` brief line."
    except (ImportError, OSError, ValueError, KeyError, TypeError, StopIteration):
        return None
    log_denial(event, "flow", reason)
    return reason


def check(event):
    """The refusal text for this spawn, or None to allow it."""
    return evaluate(event)[0]


def evaluate(event):
    """(refusal, warning) for this spawn: a refusal denies it, a warning is shown while it proceeds."""
    result = evaluate_spawn(event)
    if result[0] and result[0].startswith("Hold:"):
        try:
            north_lib()
            home = os.environ.get("SPAWN_CAPACITY_HOME")
            state = Path(home) / ".local/state" if home else Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
            provider = "claude" if event.get("tool_name") == "Agent" else "codex"
            account = billed_account(provider) or provider
            session = event.get("session_id") if not event.get("agent_id") else None
            inputs = event.get("tool_input") or {}
            task = (inputs.get("task_name") or inputs.get("name")) if isinstance(inputs, dict) else None
            pace_call("expired_grant_hold", directory=str(state / "agents"), account=account, session=session, reason=result[0], task=task)
        except (OSError, ValueError, KeyError, TypeError, AttributeError, OverflowError, RecursionError):
            pass
    return result


def evaluate_spawn(event):
    texts = brief_texts(event.get("tool_input") or {})
    provider = "claude" if event.get("tool_name") == "Agent" else "codex"
    north_lib()
    home = os.environ.get("SPAWN_CAPACITY_HOME")
    state = Path(home) / ".local/state" if home else Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
    name = billed_account(provider) or provider
    session = event.get("session_id") if not event.get("agent_id") else None
    tool_input = event.get("tool_input") or {}
    task = (tool_input.get("task_name") or tool_input.get("name")) if isinstance(tool_input, dict) else None
    grant = pace_call("grant_for", directory=str(state / "agents"), account=name, session=session, task=task)
    verdict = usage_verdict(event, grant)
    if verdict.get("refusal"):
        return verdict["refusal"], None
    name = billed_account(provider) or str((verdict.get("pace") or {}).get("account") or provider)
    if verdict.get("pace_control") and not grant:
        north_lib()
        control = (verdict.get("controls") or {}).get(name) or (verdict.get("controls") or {}).get(provider)
        if control is None:
            return f"Hold: {name} pace controller has no fresh account reading; refresh before spawning.", None
        hold = pace_call("refusal", account=name, control=control, now=time.time())
        if hold:
            log_denial(event, "projection", hold)
            return hold, None
    def admitted(warning=None):
        if verdict.get("pace_control") or grant:
            home = os.environ.get("SPAWN_CAPACITY_HOME")
            state = Path(home) / ".local/state" if home else Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
            return pace_call("reserve", path=str(state / "agents/usage-gate.json"), provider=provider, account=name, session=session, task=task,
                                        tool_use_id=event.get("tool_use_id")), warning
        return None, warning
    if any(EXEMPT_MARKER in t for t in texts):
        return admitted()
    reason = capacity_refusal(event, texts)
    if reason:
        return reason, None
    provider = "claude" if event.get("tool_name") == "Agent" else "codex"
    expected, in_flight, existing, unavailable = spawn_reservation(event, verdict, provider)
    unavailable_warning = None
    if unavailable:
        name = billed_account(provider) or str((verdict.get("pace") or {}).get("account") or provider)
        account = (verdict.get("accounts") or {}).get(name) or (verdict.get("accounts") or {}).get(provider) or {}
        used, cap = number(account.get("used_pct")), number(account.get("cap_pct"))
        if cap is not None:
            hold = f"Hold: {name} reservation unavailable: {unavailable}."
            fallback = existing if existing is not None else DEFAULTS["reservation_fallback_points"]
            in_flight = local_workers(provider)
            if used is not None and cap is not None and used >= cap - fallback * (in_flight + 1):
                log_denial(event, "account", hold)
                return hold, None
            unavailable_warning = NOT_BLOCKED + hold.removeprefix("Hold: ")
            expected, existing = fallback, fallback
        else:
            expected, in_flight, existing = 0.0, 0, 0.0
    hold = account_reservation(event, verdict, provider, expected, in_flight, existing)
    if hold:
        log_denial(event, "account", hold)
        return hold, None
    return admitted(unavailable_warning)


def capacity_refusal(event, texts):
    flow_reason = flow_gate(event, texts)
    if flow_reason:
        return flow_reason
    if not is_local(event):
        return None
    tool_input = event.get("tool_input") if isinstance(event.get("tool_input"), dict) else {}
    if any(LANDING_PATH.search(t) for t in texts) or LANDER.search(str(tool_input.get("name") or "")):
        return None
    reading = capacity()
    if reading is None:
        return None
    provisioned, limit, protected = reading
    rank = fleet_domain_rank(event)
    ceiling = limit * (1 - PRIORITY_HEADROOM) if rank and rank[1] > 0 else limit
    if provisioned < ceiling and protected <= PROTECTED_PRESSURE_LIMIT:
        return None
    fact = urgent_file_fact(texts)
    if fact:
        log_override(event, fact)
        return None
    urgent = next((m for m in map(URGENT.search, texts) if m), None)
    if urgent:
        fact = urgent.group(1) if urgent.group(1) is not None else urgent.group(2)
        if len(fact.strip()) >= 15:
            log_override(event, fact.strip())
            return None
    log_denial(event, "capacity")
    if provisioned < limit and protected <= PROTECTED_PRESSURE_LIMIT:
        return (
            f"Domain {rank[0]} ranks below {rank[2]} in Tom's priority order, so its local "
            f"spawns stop at {ceiling:g} of the {limit:g} CPUs; {provisioned:g} are committed. Move code-only work "
            "to a cloud worker (workers skill) or queue it until a lease ends. Take a real conflict to the "
            "proxy, not Tom."
        )
    return (
        f"Capacity leases and unleased heavy load commit {provisioned:g} of the {limit:g} CPUs the machine can hand out, "
        f"protected desktop pressure is {protected:g}% (limit {PROTECTED_PRESSURE_LIMIT:g}%), and "
        f"{local_workers()} local workers are running. Queue this worker until a lease ends, "
        "send heavy checks to the farm, or use a cloud worker for code-only work "
        "(workers skill). Workers already running are unaffected. For an urgent fix, "
        "start the brief with CASE=URGENT FACT=\"<why it can't wait>\". Codex, whose briefs are encrypted, appends \"<epoch>\\t<agent name>\\t<fact>\" to ~/.local/state/agents/spawn-urgent.tsv first."
    )


def main():
    try:
        event = json.loads(sys.stdin.read() or "{}")
    except ValueError:
        return 65
    reason, warning = evaluate(event) if isinstance(event, dict) else (None, None)
    if reason:
        print(json.dumps({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        }))
    elif warning:
        print(json.dumps({
            "systemMessage": warning,
            "hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": warning},
        }))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
