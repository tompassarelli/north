#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PYTHON="${NORTH_AGENT_PYTHON:-python3}"
"$PYTHON" "$HERE/lib/shellcmd.property.py" --guard generated-policy
exec "$PYTHON" - "$HERE/generated-policy-guard.sh" <<'PY'
import json, os, subprocess, sys, tempfile
from pathlib import Path

guard = sys.argv[1]
with tempfile.TemporaryDirectory() as temporary:
    home = Path(temporary)
    src = home / "src"
    src.mkdir()
    for leaf in ("host.nix", "host.bnix", "plain.nix"):
        (src / leaf).touch()
    active = home / "activation.active"
    active.write_text("hook generated-policy-guard\n")
    env = dict(os.environ, HOME=str(home), NORTH_AGENT_ACTIVE=str(active), AGENT_NO_AUTHORING_HOOKS="0")

    def denied(tool, tool_input, extra_env=None):
        payload = {"tool_name": tool, "cwd": str(src), "tool_input": tool_input}
        result = subprocess.run([guard], input=json.dumps(payload), text=True, capture_output=True,
                                env=dict(env, **(extra_env or {})), timeout=30, check=True)
        out = json.loads(result.stdout or "{}").get("hookSpecificOutput") or {}
        return out.get("permissionDecision") == "deny", out.get("permissionDecisionReason", "")

    claude = f"{home}/.claude/CLAUDE.md"
    cases = [
        (True, "agents sync", "Edit", {"file_path": claude, "old_string": "a", "new_string": "b"}),
        (True, "agents sync", "Write", {"file_path": f"{home}/.codex/skills/x/SKILL.md", "content": "x"}),
        (True, "agents sync", "NotebookEdit", {"notebook_path": f"{home}/.codex/AGENTS.md"}),
        (True, ".bnix", "apply_patch", {"input": "*** Begin Patch\n*** Update File: host.nix\n@@\n-a\n+b\n*** End Patch"}),
        (True, ".bnix", "Bash", {"command": "sed -i s/a/b/ host.nix"}),
        (True, "agents sync", "Bash", {"command": "echo x > ~/.claude/CLAUDE.md"}),
        (True, "agents sync", "Bash", {"command": "printf x >>$HOME/.codex/AGENTS.md"}),
        (False, "", "Read", {"file_path": claude}),
        (False, "", "Bash", {"command": "cat ~/.claude/CLAUDE.md"}),
        (False, "", "Bash", {"command": "echo 'x > ~/.claude/CLAUDE.md'"}),
        (False, "", "Bash", {"command": 'git commit -m "sed -i s/a/b/ host.nix"'}),
        (False, "", "Edit", {"file_path": str(src / "plain.nix")}),
        (False, "", "Bash", {"command": "agents sync"}),
        (False, "", "Write", {"file_path": f"{home}/.codex/config.toml"}),
    ]
    failures = 0
    for want, remedy, tool, tool_input in cases:
        got, reason = denied(tool, tool_input)
        if got != want or remedy not in reason:
            failures += 1
            print(f"FAIL want={want} got={got} {tool} {tool_input} {reason!r}", file=sys.stderr)
    if denied("Edit", {"file_path": claude}, {"AGENT_NO_AUTHORING_HOOKS": "1"})[0]:
        failures += 1
        print("FAIL kill-switch still denies", file=sys.stderr)
    print(f"generated-policy-guard: {len(cases) + 1 - failures}/{len(cases) + 1} tool fixtures")
    sys.exit(1 if failures else 0)
PY
