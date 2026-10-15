from datetime import datetime, timedelta, timezone
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time

HERE = Path(__file__).resolve().parent
WRAPPER = HERE.parents[1] / "bin/claude"

with tempfile.TemporaryDirectory(prefix="claude.command_") as temporary:
    home = Path(temporary)
    native = home / ".local/lib/claude/current/bin/claude"
    native.parent.mkdir(parents=True)
    native.write_text('#!/usr/bin/env python3\nimport json,os,sys\n'
                      'print(json.dumps({"args":sys.argv[1:],"profile":os.environ.get("CLAUDE_CONFIG_DIR")}))\n')
    native.chmod(0o755)
    directories = {"a": home / "profile-a", "b": None, "c": home / "profile-c"}
    reset = (datetime.now(timezone.utc) + timedelta(days=4)).isoformat()
    for label, directory in directories.items():
        config = directory or home
        config.mkdir(parents=True, exist_ok=True)
        credentials = directory or home / ".claude"
        credentials.mkdir(parents=True, exist_ok=True)
        (credentials / ".credentials.json").write_text(json.dumps({"claudeAiOauth": {
            "subscriptionType": "max", "rateLimitTier": "default_claude_max_5x" if label == "c" else "default_claude_max_20x"}}))
        cache = {"accountUuid": "fixture-" + label, "fetchedAtMs": time.time() * 1000, "utilization": {"limits": [
            {"kind": kind, "percent": (10 if label == "c" else 96) if kind == "weekly_all" else 1, "resets_at": reset}
            for kind in ("weekly_all", "session")]}}
        (config / ".claude.json").write_text(json.dumps({"oauthAccount": {"accountUuid": "fixture-" + label}, "cachedUsageUtilization": cache}))
    host = socket.gethostname()
    labels = ', '.join(f'"{hashlib.sha256(("fixture-" + label).encode()).hexdigest()[:8]}" = "{label}"' for label in directories)
    profiles = ', '.join(f'{label} = {{ "{host}" = {{ ' + (f'path = "{directory}"' if directory else '') + ' } }' for label, directory in directories.items())
    policy = home / "orchestration.toml"
    policy.write_text(f'[accounts.claude]\nlabels = {{ {labels} }}\nprofiles = {{ {profiles} }}\n'
                      'plans = { a = "max-20x", b = "max-20x", c = "max-5x" }\nauto_route_requires = { c = "max-5x" }\n')
    env = dict(os.environ, HOME=str(home), AGENTS_ORCHESTRATION=str(policy), THREADS_DB=str(home / "usage.db"), RESIDENT_CAP_HOST="claude-command-test")
    for key in ("CLAUDE_CONFIG_DIR", "AGENT_ROLE", "AGENT_ORG_NAME"):
        env.pop(key, None)

    def run(*args, extra=None):
        return subprocess.run([str(WRAPPER), *args], cwd=home, env=env | (extra or {}), capture_output=True, text=True)

    def cache(label, weekly, session=1, fetched=None):
        path = (directories[label] or home) / ".claude.json"
        data = json.loads(path.read_text())
        data["cachedUsageUtilization"]["fetchedAtMs"] = fetched or time.time() * 1000
        for row in data["cachedUsageUtilization"]["utilization"]["limits"]:
            row["percent"] = weekly if row["kind"] == "weekly_all" else session
        path.write_text(json.dumps(data))

    before = time.monotonic()
    result = run()
    elapsed = (time.monotonic() - before) * 1000
    assert result.returncode == 0 and json.loads(result.stdout)["profile"] == str(directories["c"]), result
    assert "using account c (weekly 10% of Claude Max 5x)" in result.stderr, result
    print(f"plain Claude selection and exec: {elapsed:.1f} ms")
    args = ["-p", "reply with the word ok", "--output-format", "json"]
    result = run("--account", "c", *args)
    assert json.loads(result.stdout) == {"args": args, "profile": str(directories["c"])}, result
    result = run("c", *args)
    assert json.loads(result.stdout) == {"args": args, "profile": str(directories["c"])}, result
    result = run("a")
    assert json.loads(result.stdout) == {"args": [], "profile": str(directories["a"])}, result
    result = run("cat", *args)
    assert json.loads(result.stdout) == {"args": ["cat", *args], "profile": None}, result
    result = run("--account=b", *args, extra={"CLAUDE_CONFIG_DIR": str(directories["c"])})
    assert json.loads(result.stdout) == {"args": args, "profile": None}, result
    result = run(*args)
    assert json.loads(result.stdout) == {"args": args, "profile": None} and not result.stderr, result
    result = run("--model", "opus", extra={"AGENT_ROLE": "lead", "CLAUDE_CONFIG_DIR": str(directories["c"])})
    assert json.loads(result.stdout)["profile"] == str(directories["c"]) and not result.stderr, result
    result = run("--account", "missing")
    assert result.returncode == 2 and "valid labels: a, b, c" in result.stderr, result
    cache("c", 96)
    result = run()
    assert result.returncode == 1 and all(label + ": weekly resets " + reset in result.stderr for label in directories), result
    cache("c", 10, session=95)
    assert run().returncode == 1
    cache("a", 80)
    cache("c", 10)
    assert json.loads(run().stdout)["profile"] == str(directories["c"])
    cache("a", 70)
    assert json.loads(run().stdout)["profile"] == str(directories["a"])
    cache("a", 77.5)
    assert json.loads(run().stdout)["profile"] == str(directories["a"])
    cache("a", 96)
    (directories["c"] / ".credentials.json").write_text(json.dumps({"claudeAiOauth": {"subscriptionType": "free"}}))
    assert run().returncode == 1
    assert run("--account", "c", *args).returncode == 0
    (directories["c"] / ".credentials.json").write_text(json.dumps({"claudeAiOauth": {"rateLimitTier": "default_claude_max_5x"}}))
    with sqlite3.connect(env["THREADS_DB"]) as db:
        db.execute("CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, resets_at TEXT)")
        db.execute("INSERT INTO usage VALUES (?, 'claude', 'c', 'window', 'weekly_all', 96, ?)",
                   ((datetime.now(timezone.utc) + timedelta(minutes=1)).isoformat(), reset))
    assert run().returncode == 1
    cache("c", 10, fetched=(time.time() + 120) * 1000)
    assert json.loads(run().stdout)["profile"] == str(directories["c"])
    project = ''.join(character if character.isalnum() else '-' for character in str(home))
    sessions = directories["c"] / "projects" / project
    sessions.mkdir(parents=True)
    session = "00000000-0000-4000-8000-000000000001"
    (sessions / (session + ".jsonl")).write_text("{}\n")
    for args in (("--resume", session), ("--resume",), ("-c",)):
        result = run(*args)
        assert json.loads(result.stdout) == {"args": list(args), "profile": str(directories["c"])}, result
    data = json.loads((directories["c"] / ".claude.json").read_text())
    data["oauthAccount"]["accountUuid"] = "fixture-a"
    (directories["c"] / ".claude.json").write_text(json.dumps(data))
    assert run("--account", "c").returncode != 0
    print("PASS Claude account argv, headroom, confirmed plans, freshest meters, resume ownership and re-login detection")
