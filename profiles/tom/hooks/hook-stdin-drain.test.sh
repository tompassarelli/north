#!/usr/bin/env bash
# Cross-hook regression: installed managed hooks must consume complete delayed
# envelopes before disabled and fast-allow exits.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/hook-stdin-drain-test.XXXXXX")"
trap 'rm -rf "${SCRATCH:?}"' EXIT

HOME_DIR="$SCRATCH/home"
PLAIN_DIR="$SCRATCH/plain"
STATE="$SCRATCH/harness.conf"
ACTIVATION="$SCRATCH/activation.json"
mkdir -p "$HOME_DIR" "$PLAIN_DIR" "$SCRATCH/session-state"
touch "$PLAIN_DIR/notes.txt"
printf '%s\n' 'dispatch=native' 'guards=on' >"$STATE"

hooks=(
  agent-spawn-guard.sh
  launch-critical-worktree-guard.sh
  git-blind-stage-guard.sh
  tripwire-guard.sh
  beagle-session-start.sh
  logcompress-hook.js
)
sizes=(524288 1048576)
modes=(disabled fast-allow)

pass=0
fail=0
for mode in "${modes[@]}"; do
  for size in "${sizes[@]}"; do
    for hook in "${hooks[@]}"; do
      result="$(python3 - "$HERE/$hook" "$mode" "$size" "$HOME_DIR" "$PLAIN_DIR" "$STATE" "$ACTIVATION" <<'PY'
import json
import os
import subprocess
import sys
import time

hook, mode, size_text, home, plain, state, activation = sys.argv[1:]
size = int(size_text)
name = os.path.basename(hook)
target = os.path.join(plain, "notes.txt")

envelopes = {
    "agent-spawn-guard.sh": {
        "hook_event_name": "PreToolUse",
        "tool_name": "Agent",
        "tool_input": {
            "subagent_type": "general-purpose",
            "prompt": "transport allow",
        },
        "cwd": plain,
    },
    "launch-critical-worktree-guard.sh": {
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": {"file_path": target},
        "cwd": plain,
    },
    "git-blind-stage-guard.sh": {
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "git status"},
        "cwd": plain,
    },
    "tripwire-guard.sh": {
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "true"},
        "cwd": plain,
    },
    "beagle-session-start.sh": {
        "hook_event_name": "SessionStart",
        "session_id": f"drain-{mode}-{size}",
        "source": "startup",
        "cwd": plain,
    },
    "logcompress-hook.js": {
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "true"},
        "tool_response": {
            "stdout": "ok",
            "stderr": "",
            "interrupted": False,
            "isImage": False,
            "noOutputExpected": False,
        },
        "cwd": plain,
    },
}
envelope = dict(envelopes[name])
envelope["padding"] = ""
base = json.dumps(envelope, separators=(",", ":")).encode()
padding_size = size - len(base)
if padding_size < 0:
    raise SystemExit("target envelope smaller than fixture")
envelope["padding"] = "x" * padding_size
payload = json.dumps(envelope, separators=(",", ":")).encode()
assert len(payload) == size

env = os.environ.copy()
for key in ("BASH_ENV", "ENV", "CLAUDE_PROJECT_DIR"):
    env.pop(key, None)
if mode == "disabled":
    env.pop("AGENT_NO_AUTHORING_HOOKS", None)
    hook_id = os.path.splitext(name)[0]
    category = "authoring" if hook_id in {
        "launch-critical-worktree-guard", "git-blind-stage-guard", "tripwire-guard"
    } else "context"
    with open(activation, "w", encoding="utf-8") as handle:
        json.dump({"schema": "north.agent-activation/v1", "units": [
            {"id": hook_id, "kind": "hook", "category": category, "active": False}
        ]}, handle)
else:
    try:
        os.unlink(activation)
    except FileNotFoundError:
        pass
    with open(state, "w", encoding="utf-8") as handle:
        handle.write("dispatch=native\nguards=on\n")
env.update({
    "HOME": home,
    "BEAGLE_HOME": os.path.join(os.environ["HOME"], "code/beagle/main"),
    "NORTH_HOME": os.path.abspath(os.path.join(os.path.dirname(hook), "../../..")),
    "NORTH_HARNESS_STATE": state,
    "NORTH_AGENT_ACTIVATION": activation,
    "BEAGLE_SESSION_STATE_DIR": os.path.join(home, "session-state"),
    "AGENT_TOPOLOGY": "orchestrator",
})
if mode != "disabled":
    env["AGENT_NO_AUTHORING_HOOKS"] = "0"

process = subprocess.Popen(
    [hook],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    env=env,
)
broken = False
assert process.stdin is not None
for offset in range(0, len(payload), 8192):
    try:
        process.stdin.write(payload[offset:offset + 8192])
        process.stdin.flush()
    except BrokenPipeError:
        broken = True
        break
    if offset == 0:
        time.sleep(0.05)
    else:
        time.sleep(0.001)
try:
    process.stdin.close()
except BrokenPipeError:
    broken = True
assert process.stdout is not None
assert process.stderr is not None
stdout = process.stdout.read()
stderr = process.stderr.read()
try:
    status = process.wait(timeout=8)
except subprocess.TimeoutExpired:
    process.kill()
    status = process.wait()
    broken = True

ok = not broken and status == 0 and stdout == b"" and stderr == b""
print(
    f"{'PASS' if ok else 'FAIL'}  {name} {mode} bytes={size} "
    f"status={status} broken={str(broken).lower()} "
    f"stdout={len(stdout)} stderr={len(stderr)}"
)
raise SystemExit(0 if ok else 1)
PY
)"
      if [ "$?" -eq 0 ]; then
        pass=$((pass + 1))
      else
        fail=$((fail + 1))
      fi
      printf '%s\n' "$result"
    done
  done
done

# Keep representative decision paths pinned while transport mechanics change.
printf '%s\n' 'dispatch=managed' 'guards=on' >"$STATE"
printf '%s\n' '{"schema":"north.agent-activation/v1","units":[{"id":"agent-spawn-guard","kind":"hook","category":"dispatch","active":true},{"id":"tripwire-guard","kind":"hook","category":"authoring","active":true}]}' >"$ACTIVATION"
agent_out="$(
  python3 -c 'import json; print(json.dumps({"tool_name":"Agent","tool_input":{"subagent_type":"general-purpose","prompt":"work"}}))' |
    env HOME="$HOME_DIR" NORTH_HARNESS_STATE="$STATE" NORTH_AGENT_ACTIVATION="$ACTIVATION" AGENT_NO_AUTHORING_HOOKS=0 \
      NORTH_HOME="$REPO" \
      "$HERE/agent-spawn-guard.sh"
)"
if jq -e '.hookSpecificOutput.permissionDecision == "deny"' <<<"$agent_out" >/dev/null; then
  pass=$((pass + 1))
  printf '%s\n' 'PASS  agent-spawn-guard.sh decision remains deny'
else
  fail=$((fail + 1))
  printf '%s\n' 'FAIL  agent-spawn-guard.sh decision changed'
fi

tripwire_status=0
tripwire_err="$(
  python3 -c 'import json,sys; print(json.dumps({"tool_name":"Bash","tool_input":{"command":"git push","cwd":sys.argv[1]}}))' "$PLAIN_DIR" |
    env -u SAFE_PUSH_ACTIVE HOME="$HOME_DIR" NORTH_HARNESS_STATE="$STATE" NORTH_AGENT_ACTIVATION="$ACTIVATION" \
      AGENT_NO_AUTHORING_HOOKS=0 TRIPWIRE_LOG_DIR="$SCRATCH/tripwire-log" \
      "$HERE/tripwire-guard.sh" 2>&1
)" || tripwire_status=$?
if [ "$tripwire_status" -eq 2 ] && [[ "$tripwire_err" == *"raw 'git push'"* ]]; then
  pass=$((pass + 1))
  printf '%s\n' 'PASS  tripwire-guard.sh decision remains hard deny'
else
  fail=$((fail + 1))
  printf 'FAIL  tripwire-guard.sh decision changed status=%s stderr=%s\n' \
    "$tripwire_status" "$tripwire_err"
fi

printf '\n%d/%d passed\n' "$pass" "$((pass + fail))"
[ "$fail" -eq 0 ]
