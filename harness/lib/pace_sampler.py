import argparse
import concurrent.futures
import fcntl
import json
import os
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
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


def reserve(path, provider, account):
    path.parent.mkdir(parents=True, exist_ok=True)
    with (path.parent / "pace-control.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return f"Hold: {account} pace sampler is refreshing; retry after its fresh reading."
        verdicts = load(path, {})
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


def codex_workers(timeout=5):
    import usage_codex
    deadline = time.monotonic() + timeout
    def read(call, notify):
        call(1, "initialize", {"clientInfo": {"name": "pace-control", "version": "1"},
                               "capabilities": {"experimentalApi": True}})
        notify({"method": "initialized", "params": {}})
        cursor, serial, count = None, 2, 0
        while True:
            result = call(serial, "thread/loaded/list", {"cursor": cursor, "limit": 100})
            serial += 1
            for thread in result["data"]:
                status = call(serial, "thread/read", {"threadId": thread, "includeTurns": False})["thread"]["status"]["type"]
                serial += 1
                if status == "active":
                    count += 1
            cursor = result.get("nextCursor")
            if cursor is None:
                return count
    pooled = Path(os.environ.get("NORTH_CODEX_CONVERSATION_HOME", os.environ.get("NORTH_CODEX_POOLED_HOME", Path.home() / ".local/state/north/codex-pooled")))
    endpoint = usage_codex.shared_socket(pooled, timeout=timeout)
    if endpoint is None:
        return None
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        return None
    return usage_codex.shared_usage(endpoint, read, timeout=remaining)


def workers_for(account, timeout=5):
    provider, _, label = account.partition(":")
    try:
        if provider == "codex":
            return codex_workers(timeout)
        if provider == "claude":
            from claude_profiles import config_dir, profiles
            profile = profiles().get(label)
            if profile is None:
                return None
            directory = config_dir(profile)
            env = dict(os.environ)
            env.pop("CLAUDE_CONFIG_DIR", None)
            if directory is not None:
                env["CLAUDE_CONFIG_DIR"] = str(directory)
            code, out, _ = run(["claude", "--account", label, "agents", "--json"], timeout=timeout, env=env)
            if code:
                return None
            active = sum(s["status"] == "busy" for s in json.loads(out))
            root = directory or Path.home() / ".claude"
            cutoff = time.time() - 300
            return active + sum(p.stat().st_mtime >= cutoff for p in (root / "projects").glob("*/*/subagents/*.jsonl"))
    except (OSError, ValueError, KeyError, sqlite3.Error, subprocess.SubprocessError, RuntimeError):
        return None
    return None


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


def send_codex(session, line):
    return run([str(usage.HERE.parents[1] / "bin/codex-lead"), "send", line], timeout=10,
               env=dict(os.environ, CODEX_LEAD_THREAD=session))


def deliver(account, control):
    provider, _, label = account.partition(":")
    handoffs = state_dir() / "handoffs"
    action = "HOLD-SPAWNS" if control["held"] else "RESUME-SPAWNS"
    numbers = "; ".join(f"{w['window']} used={w['used_pct']:.1f}% burn={w['burn_pct_h']}%/h reset={w['reset_h']:.2f}h projected={w['projected_pct']}%" for w in control["windows"])
    line = f"{action} {account} allowed={control['allowed_workers']} {numbers}"
    proxy = handoffs / "proxy-inbox.md"
    append(proxy, ("INCIDENT burn " if control["held"] else "") + line)
    nodes = load(Path(os.environ.get("AGENTS_ORG_FILE", state_dir() / "org.json")), {}).get("nodes", [])
    sends = []
    for node in nodes:
        if node.get("state") != "active" or node.get("provider") != provider:
            continue
        if provider == "claude" and node.get("account") and node["account"] != label:
            continue
        if node.get("status_file"):
            status = Path(node["status_file"])
            name = status.name.removesuffix("-status.md")
            append(status.with_name(name + "-inbox.md"), line)
        if provider == "codex" and node.get("session"):
            sends.append(node)
    def send(node):
        try:
            code, _, _ = send_codex(node["session"], line)
        except OSError:
            code = 127
        if code:
            append(proxy, f"DELIVERY-FAILED pace {node['id']} {action}; inbox retained")
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, len(sends))) as pool:
        list(pool.map(send, sends))
    code, _, _ = run([str(usage.HERE.parents[1] / "bin/supply-alert"), "normal", line, f"Pace: {account} {action}"], timeout=15)
    if code:
        append(proxy, f"DELIVERY-FAILED pace ntfy {account}; inbox retained")


def sample(provider=None, initial=2, notify=True):
    directory = state_dir()
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / "pace-control.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        state = load(directory / "pace-control.json", {"accounts": {}, "providers": {}})
        at = time.time()
        deadline = time.monotonic() + 90
        adapters = {"claude": "claude", "codex": "codex", "gemini": "agy"}
        due = {p: usage.HERE / f"usage_{a}.py" for p, a in adapters.items()
               if (provider is None or provider == p) and at >= state["providers"].get(p, {}).get("next_at", 0)}
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            reads = dict(zip(due, pool.map(read_adapter, due.values())))
        with usage.open_readout_db() as db:
            rows = [row for rows, _ in reads.values() for row in rows]
            usage.record(db, rows)
            if usage.REMOTE and rows:
                code, _, _ = run([str(usage.HERE.parents[1] / "bin/threads"), "ingest"], timeout=10,
                                 input="".join(json.dumps(dict(row, type="usage")) + "\n" for row in rows))
                if code:
                    append(directory / "pace-control.jsonl", json.dumps({"at": at, "ingest_code": code, "retained": "pace-control.json history"}))
            for p, (rows, measured) in reads.items():
                old = state["providers"].get(p, {})
                cadence = pace_control.interval(old.get("interval_s", 60), measured["elapsed_s"], measured["throttled"] or measured["code"] != 0)
                state["providers"][p] = measured | {"interval_s": cadence, "next_at": at + cadence}
                append(directory / "pace-control.jsonl", json.dumps({"provider": p, "at": at, **state["providers"][p]}))
            history = state.get("history", [])
            usage.record(db, history)
            verdicts = usage.write_gate(db, adapters, pace_locked=True)
            accounts = {account: (p, windows) for p, verdict in verdicts.items() if p in reads
                        for account, windows in verdict.get("projections", {}).items()}
            labels = {account: account if ":" in account else f"{p}:{next((r['account'] for r in usage.latest(db) if r['provider'] == p), '')}"
                      for account, (p, _) in accounts.items()}
            remaining = deadline - time.monotonic()
            by_label = worker_counts(list(labels.values()), min(5, remaining)) if remaining > 0 else {}
            counts = {account: by_label.get(label) for account, label in labels.items()}
            deliveries = {}
            for p, verdict in verdicts.items():
                if p not in reads:
                    continue
                controls = {}
                for account, windows in verdict.get("projections", {}).items():
                    old = state["accounts"].get(account, {})
                    control = pace_control.advance(old, windows, counts[account], time.time(), initial)
                    controls[account] = control
                    state["accounts"][account] = control
                    append(directory / "pace-control.jsonl", json.dumps({"account": account, **control}))
                    if notify and control["held"] != old.get("held"):
                        deliveries[account] = control
                verdict["controls"] = controls
                verdict["pace_control"] = True
            cutoff = time.time() - 10800
            state["history"] = [r for r in history + [r for rows, _ in reads.values() for r in rows]
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
    return verdicts


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--provider", choices=("claude", "codex", "gemini"))
    parser.add_argument("--initial", type=int, default=2)
    parser.add_argument("--dry-run", action="store_true", help="sample and log without sending account controls or notifications")
    args = parser.parse_args()
    if args.initial < 1:
        parser.error("--initial must be positive")
    sample(args.provider, args.initial, not args.dry_run)


if __name__ == "__main__":
    main()
