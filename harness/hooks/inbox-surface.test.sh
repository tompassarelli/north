#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
export PATH="/etc/codex/hooks/runtime:$PATH"
exec python3 - "$HERE" <<'PY'
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib

here = Path(sys.argv[1])
sys.path.insert(0, str(here.parent / "lib"))
import events

with tempfile.TemporaryDirectory(prefix="inbox-surface-test.") as scratch:
    root = Path(scratch)
    active = root / "active"
    active.write_text("hook inbox-surface\n")
    registry = root / "org.json"
    node = dict(id="lead", session="current", status_file=str(root / "lead-status.md"), provider="claude", state="active")
    registry.write_text(json.dumps(dict(nodes=[node])))
    env = dict(os.environ, AGENT_ORG_NAME="lead", AGENTS_ORG_FILE=str(registry),
               NORTH_AGENT_ACTIVE=str(active), XDG_RUNTIME_DIR=str(root),
               NORTH_AGENT_PYTHON=sys.executable)
    env.pop("AGENT_NO_AUTHORING_HOOKS", None)
    os.environ["AGENTS_ORG_FILE"] = str(registry)
    inbox = events.inbox_path(node)
    inbox.write_text("old history\n")

    def register(session):
        node["session"] = session
        registry.write_text(json.dumps(dict(nodes=[node])))

    def call(session="current", event="PostToolUse", payload=None, **overrides):
        result = subprocess.run([str(here / "inbox-surface.sh")],
                                input=json.dumps(dict(session_id=session, hook_event_name=event) | (payload or {})),
                                text=True, capture_output=True, env=env | overrides, check=True)
        assert not result.stderr, result.stderr
        return json.loads(result.stdout)["hookSpecificOutput"] if result.stdout else None

    assert call() is None
    events.deliver(node, "appended event")
    cursors = {path.name: path.read_bytes() for path in (root / "north-inbox-surface").iterdir()}
    assert call(payload=dict(agent_id="worker")) is None
    assert {path.name: path.read_bytes() for path in (root / "north-inbox-surface").iterdir()} == cursors
    print("PASS worker payload with the lead session prints nothing and leaves its cursor untouched")
    assert call("foreign") is None
    assert {path.name: path.read_bytes() for path in (root / "north-inbox-surface").iterdir()} == cursors
    print("PASS foreign session prints nothing and leaves all cursors untouched")
    out = call(event="UserPromptSubmit")
    assert out == dict(hookEventName="UserPromptSubmit", additionalContext="appended event\n"), out
    assert call() is None
    print("PASS appended event surfaced once; second call prints nothing")
    register("fresh")
    assert call("fresh") is None
    print("PASS fresh session starts at the current end")
    inbox.unlink()
    register("current")
    assert call() is None
    print("PASS missing inbox prints nothing")
    inbox.write_bytes(b"")
    register("large")
    assert call("large") is None
    inbox.write_bytes(b"x" * 20000)
    out = call("large")
    assert out["additionalContext"] == "x" * 16384 + "\n[inbox: 3616 bytes cut]", out
    assert call("large") is None
    print("PASS oversize input capped at 16384 bytes; reports 3616 bytes cut")
    events.deliver(node, "next event")
    assert call("large", AGENT_NO_AUTHORING_HOOKS="1") is None
    assert call("large")["additionalContext"] == "next event\n"
    assert call(AGENT_ORG_NAME="") is None
    assert call("large", XDG_RUNTIME_DIR=str(root / "missing" / "runtime")) is None
    print("PASS kill-switch and errors fail open without consuming the next event")

    spec = importlib.util.spec_from_file_location("policy_contract", here.parent / "scripts/agent-policy-contract.py")
    contract_module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(contract_module)
    policy = tomllib.loads((here.parent / "policy-owners.toml").read_text())
    guard = next(g for g in policy["guard"] if g["unit"] == "inbox-surface")
    assert guard["claude"] == ["PostToolUse:", "UserPromptSubmit:"]
    policy = dict(guard=[guard])
    catalog = root / "catalog.json"
    catalog.write_text(json.dumps(dict(
        activation={"inbox-surface": dict(distributions=[dict(type="hook", targets=["claude"])])},
        registrations={"inbox-surface": dict(owner=dict(path="harness/hooks/inbox-surface.sh"))},
        providerSupport=[])))
    claude = root / "claude.json"
    claude.write_text(contract_module.render_claude(policy, {}))
    codex = root / "codex.toml"
    codex.write_text(contract_module.render_codex(policy))
    nix = root / "module.nix"
    nix.write_text("")
    contract = contract_module.Contract()
    contract_module.check_provider_bindings(contract, policy, dict(
        catalog=catalog, hooks=here, claude=claude, codex=codex, nix=nix))
    assert not contract.errors, contract.errors
    print("PASS registration contract: both Claude events and shared kill-switch")
PY
