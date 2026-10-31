#!/usr/bin/env bash
# Account labels follow the login across hosts, and a retired provider is not gated by --write-pace-gate (the table, cost lines and snapshot recovery are cargo tests in crates/north-core/src/usage).
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT
PYTHONDONTWRITEBYTECODE=1 python3 - "$HERE" "$tmp" <<'PYTEST' || exit 1
import json, os, subprocess, sys
from pathlib import Path
here, scratch = map(Path, sys.argv[1:])
policy = scratch / "accounts.toml"
sys.path.insert(0, str(here))
import usage, usage_claude
from ledger_context import account_id
from unittest.mock import patch
policy.write_text('[accounts.claude]\nlabels = { "9b3a7fb5" = "a", "b04cb253" = "b" }\n')
os.environ.update(AGENTS_ORCHESTRATION=str(policy), CLAUDE_CONFIG_DIR=str(scratch))
cache_path = scratch / ".claude.json"
caches = {label: {"accountUuid": uuid, "utilization": {"limits": [
    {"kind": "weekly_all", "group": "weekly", "percent": pct, "resets_at": reset}]}}
    for label, uuid, pct, reset in (("a", "fixture-primary", 97, "2026-10-15T02:00:00Z"),
                                   ("b", "fixture-secondary", 98, "2026-10-17T12:00:00Z"))}
for host in ("whiterabbit", "nexus"):
    with patch("socket.gethostname", return_value=host):
        for login, cache in caches.items():
            cache_path.write_text(json.dumps({"cachedUsageUtilization": cache}))
            assert usage.local_account("claude") == login
            assert account_id("claude") == "claude-" + login
            for label, other_cache in caches.items():
                weekly = next(r for r in usage_claude.rows(other_cache, "2026-10-13T00:00:00Z") if r["kind"] == "window")
                assert weekly["account"] == label and weekly["used_pct"] == (97 if label == "a" else 98), weekly
print("PASS account labels follow the login across whiterabbit and nexus, including other-account readings")
import sqlite3
retired_db = scratch / "retired.db"
week = lambda provider, account, pct: {"ts": "2026-10-27T10:00:00Z", "provider": provider, "account": account, "kind": "window",
                                       "name": "week", "used_pct": pct, "window_min": 10080, "resets_at": "2099-01-01T00:00:00Z", "source": "fixture"}
with sqlite3.connect(retired_db) as db:
    usage.record(db, [week("codex", "main", 40), week("gemini", "main", 10), dict(week("gemini", "main", 11), ts="2026-10-27T10:30:00Z")])
stub = scratch / "north-stub"
stub.write_text("#!/usr/bin/env python3\nimport json, sys\nprint('null' if json.load(sys.stdin)['op'] == 'money_refusal' else '{}')\n")
stub.chmod(0o755)
gate_state = scratch / "retired-state"
(gate_state / "agents").mkdir(parents=True)
(gate_state / "agents/usage-gate.json").write_text(json.dumps({"codex": {"refusal": None}}))
retired_env = dict(os.environ, THREADS_DB=str(retired_db), XDG_STATE_HOME=str(gate_state), NORTH_PACE_BIN=str(stub),
                   AGENTS_OPERATOR_DIR=str(scratch / "op"))
shown = json.loads(subprocess.run(["python3", "-I", str(here / "usage.py"), "--json"], env=retired_env,
                                  capture_output=True, text=True, timeout=10, check=True).stdout)
assert [r["provider"] for r in shown] == ["gemini"], shown
table = subprocess.run(["python3", "-I", str(here / "usage.py")], env=retired_env, capture_output=True, text=True, timeout=10).stdout
assert "codex" not in table and "gemini main" in table and "2 readings" in table, table
gated = json.loads(subprocess.run(["python3", "-I", str(here / "usage.py"), "--write-pace-gate"], env=retired_env,
                                  capture_output=True, text=True, timeout=20, check=True).stdout)
assert sorted(gated) == ["claude", "gemini"], gated
assert "codex" not in json.loads((gate_state / "agents/usage-gate.json").read_text())
print("PASS a retired provider's stored codex readings are not shown by --json or the table, nor gated by --write-pace-gate")

PYTEST
echo "usage.test.sh: ok"
