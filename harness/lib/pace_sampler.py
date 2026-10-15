import argparse
import concurrent.futures
import fcntl
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pace_control
import usage


def state_dir():
    return Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state")) / "agents"


def load(path, default):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return default


def save(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as f:
        pending = Path(f.name)
        try:
            json.dump(data, f)
            f.flush()
            os.fsync(f.fileno())
            pending.replace(path)
        finally:
            pending.unlink(missing_ok=True)


def fleet_sessions():
    try:
        result = subprocess.run([os.environ.get("NORTH_BIN", "north"), "fleet", "list", "--json"],
                                capture_output=True, text=True, timeout=5, check=True)
        return json.loads(result.stdout)
    except (OSError, ValueError, subprocess.SubprocessError):
        return []


RESERVE_WAIT_S = 6.0
DEFAULTS = tomllib.loads((Path(__file__).resolve().parents[1] / "defaults.toml").read_text())
RESERVATION_TTL_S = DEFAULTS["spawn_reservation_ttl_s"]["value"]
GRANT_RESERVATION_HOLD_S = DEFAULTS["grant_reservation_hold_s"]["value"]


def locked(lock, wait):
    deadline = time.monotonic() + wait
    while True:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return True
        except BlockingIOError:
            if time.monotonic() >= deadline:
                return False
            time.sleep(0.05)


def grant_for(directory, account, session, now=None, task=None):
    now = time.time() if now is None else now
    state = load(directory / "pace-control.json", {})
    nodes = fleet_sessions()
    return next((g for g in state.get("grants", [])
                 if g["account"] == account and g["session"] == session and now < g["until"]
                 and (not g.get("task") or g["task"] == task) and g.get("uses", 1) > 0
                 and any(n.get("name") == g["lead"] and n.get("conversation") == session
                         for n in nodes)), None)


def expired_grant_hold(directory, account, session, reason, task=None):
    now = time.time()
    nodes = fleet_sessions()
    node = next((n for n in nodes if n.get("conversation") == session
                 and n.get("provider") == account.split(":")[0]
                 and (not account.startswith("claude:") or n.get("account") == account.split(":", 1)[1])), None)
    if node is None:
        return
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / "pace-control.lock").open("a") as lock:
        if not locked(lock, RESERVE_WAIT_S):
            return
        path = directory / "pace-control.json"
        state = load(path, {})
        rows = [(g, True) for g in state.get("grants", [])]
        notices = set()
        admissions = []
        audit = directory / "pace-control.jsonl"
        if audit.exists():
            with audit.open() as source:
                for line in source:
                    try:
                        row = json.loads(line)
                    except (ValueError, RecursionError):
                        continue
                    if not isinstance(row, dict):
                        continue
                    if row.get("event") == "proxy-grant":
                        rows.append((row, False))
                    elif row.get("event") == "grant-expired-holding":
                        notices.add(row.get("key"))
                    elif row.get("event") == "grant-admission":
                        admissions.append(row)
        matching = [(g, retained) for g, retained in rows if g.get("account") == account and g.get("session") == session
                    and g.get("lead") == node["name"] and g.get("authority") == "proxy"]
        if not matching:
            return
        grant, retained = max(matching, key=lambda entry: (entry[0]["at"], entry[1]))
        consumed = sum(r.get("account") == account and r.get("session") == session
                       and r.get("lead") == node["name"] and r.get("at", 0) >= grant["at"] for r in admissions)
        if (grant["until"] > now or grant.get("task") and grant["task"] != task
                or "uses" in grant and grant["uses"] <= (0 if retained else consumed)):
            return
        key = json.dumps([account, node["name"], session, grant["at"], grant["until"], grant.get("task")])
        if key in notices:
            return
        expiry = datetime.fromtimestamp(grant["until"], timezone.utc).isoformat()
        subprocess.run([os.environ.get("NORTH_BIN", "north"), "fleet", "send", node["name"],
                        f"GRANT-EXPIRED-HOLDING {node['name']} {account} expired={expiry} {reason}"],
                       capture_output=True, timeout=10)
        append(audit, json.dumps({"event": "grant-expired-holding", "at": now, "lead": node["name"],
                                 "session": session, "account": account, "until": grant["until"], "reason": reason,
                                 "key": key}))


def grant(account, maximum, until, lead, reason, task=None, uses=None):
    expiry = datetime.fromisoformat(until.replace("Z", "+00:00"))
    if expiry.tzinfo is None or expiry.utcoffset().total_seconds() != 0 or expiry.timestamp() <= time.time():
        raise ValueError("--until must be a future UTC timestamp")
    if maximum < 1 or not reason.strip() or (uses is not None and (uses < 1 or not task)):
        raise ValueError("a positive worker limit and a reason are required")
    directory = state_dir()
    nodes = fleet_sessions()
    node = next((n for n in nodes if n["name"] == lead), None)
    if not node or not node.get("conversation") or node.get("provider") != account.split(":")[0]:
        raise ValueError("--for must name an active registered lead on that provider")
    if account.startswith("claude:") and node.get("account") != account.split(":", 1)[1]:
        raise ValueError("grant account differs from the lead's billed account")
    row = {"account": account, "lead": lead, "session": node["conversation"], "max_workers": maximum,
           "until": expiry.timestamp(), "until_utc": expiry.isoformat(), "reason": reason,
           "authority": "proxy", "at": time.time(), "granted_by": os.environ.get("CODEX_THREAD_ID")}
    if task:
        row["task"] = task
    if uses is not None:
        row["uses"] = uses
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / "pace-control.lock").open("a") as lock:
        if not locked(lock, RESERVE_WAIT_S):
            raise ValueError("pace controller is busy; grant was not written")
        state = load(directory / "pace-control.json", {})
        state.setdefault("accounts", {})
        state.setdefault("providers", {})
        state["grants"] = [g for g in state.get("grants", [])
                           if g["until"] > row["at"] and (g["account"], g["lead"]) != (account, lead)] + [row]
        save(directory / "pace-control.json", state)
        append(directory / "pace-control.jsonl", json.dumps({"event": "proxy-grant", **row}))
    return row


def reservations(row):
    row["pending"] = [r if isinstance(r, dict) else {"at": r, "confirmed": True}
                      for r in row.get("pending", [])]
    return row["pending"]


def confirm(directory, session, tool_use_id, *, launched=True):
    if not tool_use_id or not (directory / "pace-control.json").exists():
        return
    with (directory / "pace-control.lock").open("a") as lock:
        if not locked(lock, RESERVE_WAIT_S):
            return
        path = directory / "pace-control.json"
        state = load(path, {})
        for row in state.get("grants", []):
            if row.get("session") != session:
                continue
            for reservation in reservations(row):
                if reservation.get("id") != tool_use_id or reservation.get("confirmed"):
                    continue
                if not launched:
                    row["pending"].remove(reservation)
                    save(path, state)
                    return
                reservation["confirmed"] = True
                if "uses" in row:
                    row["uses"] -= 1
                now = time.time()
                if now - reservation["at"] >= GRANT_RESERVATION_HOLD_S:
                    append(directory / "pace-control.jsonl", json.dumps({"event": "grant-late-confirm", "at": now,
                           "account": row["account"], "session": session, "lead": row["lead"],
                           "tool_use_id": tool_use_id, "reserved_at": reservation["at"], "uses": row.get("uses")}))
                save(path, state)
                append(directory / "pace-control.jsonl", json.dumps({"event": "grant-admission", "at": now,
                       "account": row["account"], "session": session, "lead": row["lead"],
                       "tool_use_id": tool_use_id, "reserved_at": reservation["at"]}))
                return


def reserve(path, provider, account, wait=RESERVE_WAIT_S, session=None, task=None, tool_use_id=None, reservation=None):
    path.parent.mkdir(parents=True, exist_ok=True)
    proposed = grant_for(path.parent, account, session, task=task)
    live = grant_workers(proposed) if proposed else None
    with (path.parent / "pace-control.lock").open("a") as lock:
        if not locked(lock, wait):
            return f"Hold: {account} pace controller stayed locked for {wait:.0f}s; retry after its fresh reading."
        verdicts = load(path, {})
        state_path = path.parent / "pace-control.json"
        state = load(state_path, {})
        if reservation is not None:
            verdict = verdicts.get(provider, {})
            row = (verdict.get("accounts") or {}).get(account) or (verdict.get("accounts") or {}).get(provider) or {}
            control = (verdict.get("controls") or {}).get(account) or (verdict.get("controls") or {}).get(provider) or {}
            if not pace_control.fresh(control, time.time()):
                return f"Hold: {account} pace controller reading is not fresh."
            if control.get("held_reason", "stale"):
                return f"Hold: {account} controller held: {control.get('held_reason', 'stale')}."
            used, cap = row.get("used_pct"), row.get("cap_pct")
            if used is not None and cap is not None:
                workers = control.get("workers")
                if workers is None:
                    return f"Hold: {account} live worker attribution unavailable."
                expected, existing = reservation
                if usage.reservation_stop(used, cap, expected, workers, existing):
                    return f"Hold: {account} projected account reservation exceeds cap {cap:g}%."
        grant = grant_for(path.parent, account, session, task=task)
        if grant:
            row = next(g for g in state["grants"] if g["account"] == account and g["lead"] == grant["lead"])
            retained = reservations(row)
            pending = [r for r in retained if 0 <= time.time() - r["at"] < RESERVATION_TTL_S]
            slots = [r for r in pending if not r.get("counted_live")]
            if live is not None:
                for reservation in slots[:len(set(live) - set(row.get("live", [])))]:
                    reservation["counted_live"] = True
            workers = len(live) + sum(not r.get("counted_live") for r in slots) if live is not None else None
            if workers is None or workers >= row["max_workers"]:
                return f"Hold: proxy grant for {grant['lead']} workers {workers}/{grant['max_workers']}; measured count required."
            held = [r for r in retained if not r.get("confirmed") and time.time() - r["at"] < GRANT_RESERVATION_HOLD_S]
            if "uses" in row and len(held) >= row["uses"]:
                return f"Hold: proxy grant for {grant['lead']} has all remaining uses reserved."
            row["live"], row["pending"] = sorted(live), retained + [{"at": time.time(), "id": tool_use_id}]
            save(state_path, state)
            return None
        controls = verdicts.get(provider, {}).get("controls", {})
        control = controls.get(account) or controls.get(provider)
        if control is None:
            return f"Hold: {account} pace controller has no account reading."
        reason = pace_control.refusal(account, control, time.time())
        if reason:
            return reason
        control["workers"] += 1
        save(path, verdicts)
        return None


def run(argv, timeout=45, input=None, **kwargs):
    proc = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, start_new_session=True, stdin=subprocess.PIPE if input is not None else subprocess.DEVNULL, **kwargs)
    try:
        out, err = proc.communicate(input=input, timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        out, err = proc.communicate()
        return 124, out, "timeout"
    return proc.returncode, out, err


def read_adapter(path):
    start = time.monotonic()
    code, out, err = run([sys.executable, "-I", str(path)])
    rows = []
    for line in out.splitlines():
        try:
            row = json.loads(line)
            if isinstance(row, dict):
                rows.append(row)
        except ValueError:
            pass
    return rows, {"elapsed_s": time.monotonic() - start, "code": code,
                  "throttled": "429" in err or "rate limit" in err.lower() or "throttled=true" in err.lower()}


def grant_workers(grant):
    provider, _, label = grant["account"].partition(":")
    nodes = fleet_sessions()
    ids = {grant["lead"]}
    while children := {n["name"] for n in nodes if n.get("parent") in ids} - ids:
        ids.update(children)
    return {n["conversation"] for n in nodes if n["name"] in ids and n["name"] != grant["lead"] and n.get("conversation")
            and n.get("provider") == provider and n.get("status") == "working"
            and (provider != "claude" or n.get("account") == label)}


def workers_for(account, timeout=5):
    provider, _, label = account.partition(":")
    return sum(n.get("status") == "working" and n.get("provider") == provider
               and (provider != "claude" or n.get("account") == label) for n in fleet_sessions())


def worker_counts(labels, timeout=5):
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, len(labels))) as pool:
        return dict(zip(labels, pool.map(lambda label: workers_for(label, timeout), labels)))


def append(path, line):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+b") as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        f.seek(0, os.SEEK_END)
        if f.tell():
            f.seek(f.tell() - 1)
            if f.read(1) != b"\n":
                f.write(b"\n")
        f.write((line + "\n").encode())


def deliver(account, control):
    provider, _, label = account.partition(":")
    action = "HOLD-SPAWNS" if control["held"] else "RESUME-SPAWNS"
    numbers = "; ".join(f"{w['window']} used={w['used_pct']:.1f}% burn={w['burn_pct_h']}%/h horizon={w.get('horizon_h', w['reset_h']):.2f}h projected={w['projected_pct']}%" for w in control["windows"])
    if control.get("target"):
        numbers += f" target={control['target']['spend_to_pct']}% by={control['target']['by']}"
    line = f"{action} {account} allowed={control['allowed_workers']} {numbers}"
    nodes = fleet_sessions()
    for node in nodes:
        if node.get("provider") != provider or node.get("status") == "exited":
            continue
        if provider == "claude" and node.get("account") and node["account"] != label:
            continue
        grant = grant_for(state_dir(), account, node.get("conversation"))
        if (grant and grant["lead"] == node.get("name") and grant.get("authority") == "proxy"
                and (provider != "claude" or node.get("account") == label)):
            continue
        result = subprocess.run([os.environ.get("NORTH_BIN", "north"), "fleet", "send", node["name"], line],
                                capture_output=True, timeout=10)
        if result.returncode:
            print(f"pace-control: Fleet notification failed for {node['name']}", file=sys.stderr)
    code, _, _ = run([str(usage.HERE.parents[1] / "bin/supply-alert"), "normal", line, f"Pace: {account} {action}"], timeout=15)
    if code:
        print(f"pace-control: phone notification failed for {account}", file=sys.stderr)


def sample(provider=None, initial=2, notify=True):
    directory = state_dir()
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / "pace-sampling.lock").open("a") as sampling:
        fcntl.flock(sampling, fcntl.LOCK_EX)
        at = time.time()
        deadline = time.monotonic() + 90
        adapters = {"claude": "claude", "codex": "codex", "gemini": "agy"}
        providers = load(directory / "pace-control.json", {}).get("providers", {})
        due = {p: usage.HERE / f"usage_{a}.py" for p, a in adapters.items()
               if (provider is None or provider == p) and at >= providers.get(p, {}).get("next_at", 0)}
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            reads = dict(zip(due, pool.map(read_adapter, due.values())))
        fresh = [row for rows, _ in reads.values() for row in rows]
        with usage.open_readout_db() as db:
            usage.record(db, fresh)
        if usage.REMOTE and fresh:
            code, _, _ = run([str(usage.HERE.parents[1] / "bin/threads"), "ingest"], timeout=10,
                             input="".join(json.dumps(dict(row, type="usage")) + "\n" for row in fresh))
            if code:
                append(directory / "pace-control.jsonl", json.dumps({"at": at, "ingest_code": code, "retained": "pace-control.json history"}))
        with (directory / "pace-control.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            state = load(directory / "pace-control.json", {"accounts": {}, "providers": {}})
            for p, (rows, measured) in reads.items():
                old = state["providers"].get(p, {})
                cadence = pace_control.interval(old.get("interval_s", 60), measured["elapsed_s"], measured["throttled"] or measured["code"] != 0)
                state["providers"][p] = measured | {"interval_s": cadence, "next_at": at + cadence}
                append(directory / "pace-control.jsonl", json.dumps({"provider": p, "at": at, **state["providers"][p]}))
            history = state.get("history", [])
            with usage.open_readout_db() as db:
                usage.record(db, history + (fresh if usage.REMOTE else []))
                for line in usage.burn_step(db, "codex"):
                    print(line)
                verdicts = usage.write_gate(db, adapters, pace_locked=True)
                latest = usage.latest(db)
            accounts = {account: (p, windows) for p, verdict in verdicts.items() if p in reads
                        for account, windows in verdict.get("projections", {}).items()}
            labels = {account: account if ":" in account else f"{p}:{next((r['account'] for r in latest if r['provider'] == p), '')}"
                      for account, (p, _) in accounts.items()}
        remaining = deadline - time.monotonic()
        by_label = worker_counts(list(labels.values()), min(5, remaining)) if remaining > 0 else {}
        counts = {account: by_label.get(label) for account, label in labels.items()}
        with (directory / "pace-control.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            nodes = fleet_sessions()
            state["grants"] = [g for g in load(directory / "pace-control.json", {}).get("grants", [])
                               if any(n.get("name") == g["lead"] and n.get("conversation") == g["session"]
                                      and n.get("provider") == g["account"].split(":")[0]
                                      and (not g["account"].startswith("claude:") or n.get("account") == g["account"].split(":", 1)[1])
                                      for n in nodes)]
            deliveries = {}
            for p, verdict in verdicts.items():
                if p not in reads:
                    continue
                controls = {}
                for account, windows in verdict.get("projections", {}).items():
                    old = state["accounts"].get(account, {})
                    current = time.time()
                    control = pace_control.advance(old, windows, counts[account], current, initial,
                                                   target=pace_control.target_for(account, current))
                    controls[account] = control
                    verdict["projections"][account] = control["windows"]
                    state["accounts"][account] = control
                    append(directory / "pace-control.jsonl", json.dumps({"account": account, **control}))
                    if notify and control["held"] != old.get("held"):
                        deliveries[account] = control
                verdict["controls"] = controls
                verdict["pace_control"] = True
            cutoff = time.time() - 10800
            state["history"] = [r for r in history + fresh
                                if usage.age_min(r.get("ts"), usage.datetime.now(usage.timezone.utc)) is not None
                                and usage.datetime.fromisoformat(r["ts"].replace("Z", "+00:00")).timestamp() >= cutoff]
            save(directory / "pace-control.json", state)
            save(usage.gate_path(), verdicts)
    def send(account):
        try:
            deliver(account, deliveries[account])
        except (OSError, subprocess.SubprocessError) as error:
            print(f"pace-control: delivery failed for {account}: {type(error).__name__}", file=sys.stderr)
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, len(deliveries))) as pool:
        list(pool.map(send, deliveries))
    import spawn
    spawn.release()
    return verdicts


def main():
    parser = argparse.ArgumentParser()
    if sys.argv[1:2] == ["grant"]:
        parser.add_argument("command")
        parser.add_argument("account", choices=("codex", "claude:a", "claude:b", "claude:c"))
        parser.add_argument("max_workers", type=int)
        parser.add_argument("--until", required=True)
        parser.add_argument("--for", dest="lead", required=True)
        parser.add_argument("--reason", required=True)
        parser.add_argument("--task")
        parser.add_argument("--uses", type=int)
        args = parser.parse_args()
        try:
            print(json.dumps(grant(args.account, args.max_workers, args.until, args.lead, args.reason, args.task, args.uses)))
        except (ValueError, OSError) as error:
            parser.error(str(error))
        return
    parser.add_argument("--provider", choices=("claude", "codex", "gemini"))
    parser.add_argument("--initial", type=int, default=2)
    parser.add_argument("--dry-run", action="store_true", help="sample and log without sending account controls or notifications")
    args = parser.parse_args()
    if args.initial < 1:
        parser.error("--initial must be positive")
    sample(args.provider, args.initial, not args.dry_run)


if __name__ == "__main__":
    main()
