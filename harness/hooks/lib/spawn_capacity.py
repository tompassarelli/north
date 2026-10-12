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

Delegation: a Claude Agent spawn is refused when the spawner's delegation
budget is 0. A subagent's budget is the `Delegation: ... budget=N` line of its
own brief (none: worker, 0); a session's is AGENT_DELEGATION_BUDGET, else the
default for AGENT_ROLE, and an unset role is Tom's proxy (3). Priority: a
session whose org node (AGENT_ORG_NAME in ~/.local/state/agents/org.json)
belongs to a project below the first in orchestration.toml's [projects] priority order stops at
(1 - PRIORITY_HEADROOM) of the CPU limit.

Pace hold: usage-gate.json carries routing's pace reading per account. The hold reads the account this session
bills (the catalog's host-to-account map for the spawned provider, else the routed reading); at or over pace, a
spawn whose project (brief `Item: repo#N`, else the domain of the org node named by AGENT_ORG_NAME or, for a Codex
lead whose app-server carries no AGENT_ORG_NAME, the node whose session is the spawning thread) has no live target with a `drain` grant for the account
in orchestration.toml (routing.drain_open, the rule `agents plan` routes by) is held from
the account's `[pace] enforce_from` time (a date means its UTC midnight) and warned before it, except that an account
at its drain floor (week headroom at or below its target_left, routing.exhausted) holds before it too. Tom's own session (an
interactive Claude CLI with no AGENT_ROLE, as org-role.sh defines the proxy), and a spawn with neither an Item nor an
org node, are only warned, and every warning says that nothing was blocked. A Codex spawn is never exempt as the proxy, since the shared app-server that runs Codex
hooks carries no AGENT_ROLE. A stale readout only warns.
"""

import json
import os
import re
import sys
import time
import tomllib
from pathlib import Path

PROTECTED_PRESSURE_LIMIT = 20.0
HELPER_TIMEOUT = 2.0
LOCAL_CLAUDE_TYPES = {"worker", "worker-high", "worker-xhigh", "worker-haiku", "worker-sonnet", "general-purpose", "Explore", "fork"}
ACTIVE_SECONDS = 300
EXEMPT_MARKER = "[routine:watchdog]"
ROLE_BUDGETS = {"proxy": 3, "lead": 2, "sub-lead": 1, "worker": 0}
REGISTRY = next(p for p in (Path(__file__).resolve().parent / "defaults.toml",
                             Path(__file__).resolve().parents[2] / "defaults.toml") if p.exists())
DEFAULTS = {name: entry["value"] for name, entry in tomllib.loads(REGISTRY.read_text()).items()}
PRIORITY_HEADROOM = DEFAULTS["priority_headroom"]
DELEGATION = re.compile(r"(?mi)^\W*Delegation:.*?\bbudget\s*=\s*(\d+)")
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


def capacity_probe():
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
            raw = subprocess.run([bun, str(helper), "probe", "--class", "agent"], capture_output=True,
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


def subagent_brief(event):
    """The first prompt of the subagent making this call, or None."""
    transcript, agent = event.get("transcript_path"), event.get("agent_id")
    if not (isinstance(transcript, str) and isinstance(agent, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,128}", agent)):
        return None
    path = Path(transcript).with_suffix("") / "subagents" / f"agent-{agent}.jsonl"
    try:
        with path.open(encoding="utf-8") as f:
            for raw in f:
                entry = json.loads(raw)
                if entry.get("type") == "user":
                    content = (entry.get("message") or {}).get("content")
                    if isinstance(content, list):
                        content = "\n".join(c.get("text", "") for c in content if isinstance(c, dict))
                    return content if isinstance(content, str) else None
    except (OSError, ValueError):
        return None
    return None


def delegation_budget(event):
    """(budget, where it came from) for the agent making this spawn."""
    if event.get("agent_id"):
        match = DELEGATION.search(subagent_brief(event) or "")
        if match:
            return int(match.group(1)), "the Delegation line of your brief"
        return 0, "your brief, which has no Delegation line, so you are a worker"
    raw = os.environ.get("AGENT_DELEGATION_BUDGET", "")
    if raw.isdigit():
        return int(raw), "AGENT_DELEGATION_BUDGET"
    role = os.environ.get("AGENT_ROLE") or "proxy"
    return ROLE_BUDGETS.get(role, 0), f"the default for role {role}"


def operator_dir():
    configured = os.environ.get("AGENTS_OPERATOR_DIR")
    if configured:
        return Path(configured)
    return Path.home() / "code/south/main/agents"


def orchestration_path():
    return os.environ.get("AGENTS_ORCHESTRATION") or operator_dir() / "orchestration.toml"


def org_domain_rank():
    """(domain, rank in the priority order, first domain) for this session's org node, or None."""
    domain = org_domain()
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


def usage_verdict(event):
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
            return verdict if time.time() - verdict["epoch"] < verdict["stale_min"] * 60 else None
        except (OSError, ValueError, KeyError, TypeError):
            return None

    verdict = read()
    if verdict is not None:
        return verdict
    agents = shutil.which("agents") or os.path.expanduser("~/.local/share/north/bin/agents")
    try:
        result = subprocess.run([agents, "usage", "--gate", provider], capture_output=True, text=True, timeout=45)
    except (OSError, subprocess.TimeoutExpired):
        return {}
    verdict = read() or {}
    if result.returncode == 3 and result.stdout.strip():
        verdict = dict(verdict, refusal=result.stdout.strip())
    return verdict


ITEM = re.compile(r"(?mi)^\W*Item:\s*(?:[\w-]+/)?([\w.-]+)#\d+")


def org_domain(event=None):
    name = os.environ.get("AGENT_ORG_NAME")
    session = (event or {}).get("session_id")
    if not (name or session):
        return None
    path = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state")) / "agents/org.json"
    try:
        org = json.loads(Path(os.environ.get("AGENTS_ORG_FILE", path)).read_text())
        return next(n["domain"] for n in org["nodes"]
                    if (n["id"] == name if name else n.get("session") == session)).lower()
    except (OSError, ValueError, KeyError, TypeError, StopIteration, AttributeError):
        return None


def north_lib():
    import shutil
    agents = shutil.which("agents") or str(Path.home() / ".local/share/north/bin/agents")
    libraries = [Path(__file__).resolve().parents[2] / "lib", Path(agents).resolve().parent.parent / "harness/lib"]
    library = next(path for path in libraries if (path / "usage.py").is_file())
    if str(library) not in sys.path:
        sys.path.insert(0, str(library))
    return library


def billed_account(provider):
    """The account a session on this host bills for provider, from usage's host-to-account map, or None."""
    try:
        north_lib()
        import usage
        label = usage.local_account(provider, os.environ.get("SPAWN_CAPACITY_HOST"))
    except (ImportError, OSError, ValueError, StopIteration):
        return None
    return f"{provider}:{label}" if label else None


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
        expected, _ = usage.ledger_reservation(provider, data.get("model"), data.get("reasoning_effort") or data.get("effort"),
                                              label=label)
        if expected is None:
            return None, 0, None, "no ledger-measured points per run"
        existing, _ = usage.ledger_reservation(provider, None, None, label=label)
        in_flight = local_workers(provider)
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


def pace_hold(texts, verdict, provider="claude", event=None, expected=0.0, in_flight=0, existing=None):
    """(refusal, warning) for a spawn on an account at or over its pace line whose project has no open target with
    Tom's drain grant for that account (routing.drain_open). The account holds from its `[pace] enforce_from` time
    in orchestration.toml and only warns before it or without one."""
    from datetime import datetime, timezone
    paces = verdict.get("paces") if isinstance(verdict.get("paces"), dict) else {}
    pace = paces.get(billed_account(provider)) or verdict.get("pace")
    if not isinstance(pace, dict):
        return None, None
    if pace.get("stale"):
        return None, NOT_BLOCKED + str(pace["stale"])
    used, cap = number(pace.get("used_pct")), number(pace.get("pace_pct"))
    north_lib()
    from usage import reservation_stop
    reserved = used is not None and cap is not None and expected > 0 and reservation_stop(used, cap, expected, in_flight, existing)
    if pace.get("state") != "at or over pace" and not reserved:
        return None, None
    if any(re.search(r"\bCASE=URGENT\b", text) for text in texts) or urgent_file_fact(texts):
        return None, None
    match = next((m for m in map(ITEM.search, texts) if m), None)
    project = match.group(1).lower() if match else org_domain(event)
    try:
        import tomllib
        with open(orchestration_path(), "rb") as f:
            cfg = tomllib.load(f)
    except (OSError, ValueError):
        return None, None
    projects = {k.lower(): v for k, v in cfg.get("projects", {}).items() if isinstance(v, dict)}
    account = str(pace.get("account"))
    target = projects.get(project, {}).get("target") if project else None
    missing = "has no approved deadline. Queue it, or ask Tom through the proxy for a dated goal."
    try:
        north_lib()
        import routing
        now = datetime.now(timezone.utc)
        if project and routing.drain_open(projects.get(project, {}), account, now):
            return None, None
        if project and routing.target_open(target, now):
            missing = (f"has an open target ({target}) but no drain grant for {account}. Queue it, or ask Tom "
                       "through the proxy for a drain grant.")
    except (ImportError, OSError, StopIteration):
        pass
    line = (f"Hold: {account} is at or over its pace line (used {number(pace.get('used_pct')) or 0:.1f}% vs pace "
            f"{number(pace.get('pace_pct')) or 0:.1f}%); {project or 'this project'} {missing}")
    if reserved and pace.get("state") != "at or over pace":
        line = (f"Hold: {account} used {used:g}% + reserve {(expected if existing is None else existing) * in_flight:g} points "
                f"({in_flight} in flight) + next {expected:g} points > pace {cap:g}%; "
                f"{project or 'this project'} {missing}")
    start = (cfg.get("pace") or {}).get("enforce_from", {}).get(account)
    try:
        start = start if isinstance(start, datetime) else datetime.fromisoformat(str(start).replace("Z", "+00:00"))
        enforced = datetime.now(timezone.utc) >= (start if start.tzinfo else start.replace(tzinfo=timezone.utc))
    except (ValueError, TypeError):
        enforced = False
    proxy = (event or {}).get("tool_name") == "Agent" and not event.get("agent_id") \
        and os.environ.get("CLAUDE_CODE_ENTRYPOINT") == "cli" and (os.environ.get("AGENT_ROLE") or "proxy") == "proxy"
    if pace.get("exhausted") and not enforced:
        line = (f"Hold: {account} is at its drain floor (used {number(pace.get('used_pct')) or 0:.1f}%); "
                f"{project or 'this project'} has no drain grant for it. Spawn on another account, or ask Tom through "
                "the proxy for a drain grant.")
    if (enforced or pace.get("exhausted")) and project and not proxy:
        return line, None
    return None, NOT_BLOCKED + line.removeprefix("Hold: ")


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
    texts = brief_texts(event.get("tool_input") or {})
    if any(EXEMPT_MARKER in t for t in texts):
        return None, None
    verdict = usage_verdict(event)
    if verdict.get("refusal"):
        return verdict["refusal"], None
    reason = capacity_refusal(event, texts)
    if reason:
        return reason, None
    provider = "claude" if event.get("tool_name") == "Agent" else "codex"
    expected, in_flight, existing, unavailable = spawn_reservation(event, verdict, provider)
    unavailable_warning = None
    if unavailable:
        name = billed_account(provider) or str((verdict.get("pace") or {}).get("account") or provider)
        account = (verdict.get("accounts") or {}).get(name) or (verdict.get("accounts") or {}).get(provider) or {}
        pace = (verdict.get("paces") or {}).get(name) or verdict.get("pace") or {}
        caps = [(number(account.get("used_pct")), number(account.get("cap_pct"))),
                (number(pace.get("used_pct")), number(pace.get("pace_pct")))]
        if any(cap is not None for _, cap in caps):
            hold = f"Hold: {name} reservation unavailable: {unavailable}."
            north_lib()
            import usage
            fallback = DEFAULTS["reservation_fallback_points"]
            try:
                measured, _ = usage.ledger_reservation(provider, None, None, label=name.partition(":")[2])
                if measured is not None:
                    fallback = measured
            except (ImportError, OSError, ValueError, RuntimeError, StopIteration):
                pass
            fallback *= local_workers(provider) + 1
            if any(used is not None and cap is not None and used >= cap - fallback for used, cap in caps):
                log_denial(event, "account" if number(account.get("cap_pct")) is not None else "pace", hold)
                return hold, None
            unavailable_warning = NOT_BLOCKED + hold.removeprefix("Hold: ")
        expected, in_flight, existing = 0.0, 0, 0.0
    hold = account_reservation(event, verdict, provider, expected, in_flight, existing)
    if hold:
        log_denial(event, "account", hold)
        return hold, None
    hold, warning = pace_hold(texts, verdict, provider, event, expected, in_flight, existing)
    if hold:
        log_denial(event, "pace", hold)
    if unavailable_warning:
        warning = unavailable_warning + (" " + warning.removeprefix(NOT_BLOCKED) if warning else "")
    return hold, warning


def capacity_refusal(event, texts):
    if event.get("tool_name") == "Agent":
        budget, source = delegation_budget(event)
        if budget <= 0:
            return (
                f"Your delegation budget is 0 ({source}). Do this work directly yourself: don't spawn, "
                "fork or delegate it. If it is more than you can finish, land what passes and report the rest "
                "to whoever briefed you."
            )
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
    rank = org_domain_rank()
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
            f"Domain {rank[0]} ranks below {rank[2]} in Tom's priority order (agents org show), so its local "
            f"spawns stop at {ceiling:g} of the {limit:g} CPUs; {provisioned:g} are committed. Move code-only work "
            "to a cloud worker (cloud-workers skill) or queue it until a lease ends. Take a real conflict to the "
            "proxy, not Tom."
        )
    return (
        f"Capacity leases and unleased heavy load commit {provisioned:g} of the {limit:g} CPUs the machine can hand out, "
        f"protected desktop pressure is {protected:g}% (limit {PROTECTED_PRESSURE_LIMIT:g}%), and "
        f"{local_workers()} local workers are running. Queue this worker until a lease ends, "
        "send heavy checks to the farm, or use a cloud worker for code-only work "
        "(cloud-workers skill). Workers already running are unaffected. For an urgent fix, "
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
