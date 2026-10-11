#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
HOOK="$HERE/heavy-command-guard.sh"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/heavy-command-guard-test.XXXXXX")"
trap 'rm -rf "${SCRATCH:?}"' EXIT
ACTIVATION="$SCRATCH/activation.active"
printf 'hook heavy-command-guard\n' >"$ACTIVATION"
python3 "$HERE/lib/shellcmd.property.py" --guard heavy-command
env -u AGENT_NO_AUTHORING_HOOKS NORTH_AGENT_ACTIVE="$ACTIVATION" \
  NORTH_AGENT_PYTHON=/etc/codex/hooks/runtime/python3 \
  python3 - "$HOOK" "$SCRATCH" <<'PY'
import json, os, pathlib, subprocess, sys, time

hook, scratch = sys.argv[1:]
repo = pathlib.Path(scratch) / "repo"
repo.mkdir()
subprocess.run(["git", "init", "-q", str(repo)], check=True)
(repo / "nested").mkdir()
(repo / ".agents").mkdir()
declaration = repo / ".agents" / "farm-only.toml"
declaration.write_text('''[[command]]
pattern = ["echo", "farm-me"]
narrowing = []
farm = "fixture farm echo"
why = "recorded declaration boundary"

[[command]]
pattern = ["bun", "wisp", "combos", "--advantage"]
narrowing = ["--fighter"]
farm = "fixture farm advantage"
why = "recorded whole-roster advantage sweep"

[[command]]
pattern = ["bun", "test"]
narrowing = []
farm = "fixture farm test"
why = "recorded full suite"
''')
elapsed = []

def exercise(provider, command, deny=False, override=False):
    tool_input = {"command": command} if provider == "Bash" else {"cmd": command, "workdir": str(repo / "nested")}
    payload = {"tool_name": provider, "cwd": str(repo / "nested"), "tool_input": tool_input}
    if provider == "exec_command":
        payload["tool_input"] = json.dumps(tool_input)
        payload["cwd"] = scratch
    env = dict(os.environ)
    if override:
        env["AGENT_NO_AUTHORING_HOOKS"] = "1"
    start = time.monotonic()
    result = subprocess.run([hook], input=json.dumps(payload), text=True, capture_output=True, env=env, check=True)
    elapsed.append((time.monotonic() - start) * 1000)
    response = json.loads(result.stdout) if result.stdout else {}
    output = response.get("hookSpecificOutput", {})
    assert (output.get("permissionDecision") == "deny") == deny, (provider, command, result.stdout)
    assert not result.stderr, result.stderr

for provider in ("Bash", "exec_command"):
    exercise(provider, "echo farm-me", True)
    exercise(provider, "bun wisp combos --advantage", True)
    exercise(provider, "env -u HOME CHECK=1 nix develop -c bash -lc 'echo ready && bun /opt/machine-capacity.mjs run --owner o --timeout-seconds 60 -- bun wisp combos --jobs 5 --advantage; echo done'", True)
    exercise(provider, "bun wisp combos --fighter Grom --jobs 5 --advantage")
    exercise(provider, "bun wisp combos --advantage --fighter=Grom")
    exercise(provider, "bun wisp combos --pair Grom:Chen --advantage", True)
    exercise(provider, 'echo "bun wisp combos --advantage"')
    exercise(provider, "cd .. && echo farm-me", True)
    exercise(provider, "bun test", True)
    exercise(provider, "bun test test/a.test.ts")
    exercise(provider, "echo farm-me", override=True)
declaration.unlink()
for provider in ("Bash", "exec_command"):
    exercise(provider, "echo farm-me")
    exercise(provider, "bun test", True)
declaration.write_text("[[command]\nnot toml")
for provider in ("Bash", "exec_command"):
    exercise(provider, "echo farm-me")
    exercise(provider, "bun test", True)
print("farm-only TOML/provider boundary passed")
PY
