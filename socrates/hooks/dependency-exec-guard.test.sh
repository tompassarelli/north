#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
HOOK="$HERE/dependency-exec-guard.sh"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/dependency-exec-guard-test.XXXXXX")"
trap 'rm -rf "${SCRATCH:?}"' EXIT
printf 'hook dependency-exec-guard\n' >"$SCRATCH/activation.active"
env -u AGENT_NO_AUTHORING_HOOKS NORTH_AGENT_ACTIVE="$SCRATCH/activation.active" \
  NORTH_AGENT_PYTHON=/etc/codex/hooks/runtime/python3 \
  AGENT_HOOK_ERRORS="$SCRATCH/errors.tsv" \
  python3 - "$HOOK" "$SCRATCH" <<'PY'
import json, os, pathlib, subprocess, sys

hook, scratch = sys.argv[1:]
counts = {"deny": 0, "pass": 0}

def exercise(provider, command=None, alternative=None, raw=None, overrides=None):
    tool_input = {"command": command} if provider == "Bash" else {"cmd": command, "workdir": scratch}
    if provider != "Bash":
        tool_input = json.dumps(tool_input)
    payload = raw if raw is not None else json.dumps({"tool_name": provider, "cwd": scratch, "tool_input": tool_input})
    result = subprocess.run([hook], input=payload, text=True, capture_output=True,
                            env=dict(os.environ, **(overrides or {})), check=True)
    output = json.loads(result.stdout).get("hookSpecificOutput", {}) if result.stdout else {}
    assert (output.get("permissionDecision") == "deny") == bool(alternative), (provider, command, result.stdout)
    if alternative:
        assert alternative in output["permissionDecisionReason"], (command, output)
        assert output["hookEventName"] == "PreToolUse", output
    assert not result.stderr, result.stderr
    counts["deny" if alternative else "pass"] += 1

scenarios = []
for action in ("install", "i"):
    scenarios.append((f"bun {action}", "fetch -- bun install --frozen-lockfile --ignore-scripts"))
for action in ("add left-pad", "update", "remove left-pad", "rm left-pad", "link", "pm trust", "x tool"):
    scenarios.append((f"bun {action}", f"fetch -- bun {action}"))
for manager in ("bunx", "npx", "npm", "pnpm", "yarn"):
    scenarios.append((f"{manager} tool", f"fetch -- {manager} tool"))
for action in ("build", "b", "test", "t", "check", "c", "clippy", "run", "r", "bench", "doc"):
    scenarios.append((f"cargo {action}", f"run -- cargo {action} --locked --offline"))
for action in ("fetch", "update", "add thing", "install thing"):
    scenarios.append((f"cargo {action}", f"fetch -- cargo {action}"))
scenarios.extend([
    ("cd /tmp && CHECK=1 env -u HOME bun add left-pad", "fetch -- bun add left-pad"),
    ("echo ready; bun add left-pad", "fetch -- bun add left-pad"),
    ("echo ready | bun add left-pad", "fetch -- bun add left-pad"),
    ("depbox fetch -- bun add one && bun add two", "fetch -- bun add two"),
    ("nix develop -c cargo test", "run -- nix develop -c cargo test --locked --offline"),
    ("nix shell nixpkgs#cargo --command cargo check", "run -- nix shell 'nixpkgs#cargo' --command cargo check --locked --offline"),
    ("env -C /tmp CHECK=1 nix develop -c bash -lc 'cargo test'", "run -- nix develop -c bash -lc 'cargo test --locked --offline'"),
    ("echo ready # npm install; cargo build\nnix shell nixpkgs#cargo --command cargo check", "run -- nix shell 'nixpkgs#cargo' --command cargo check --locked --offline"),
    ("cargo run -- arg", "run -- cargo run --locked --offline -- arg"),
])
passes = ["echo ready # npm install; cargo build", "echo '#' cargo build", "echo ';' npm install", "echo '|' cargo build", "bun test", "bun run X", "bun script.ts", "bun pm ls", "cargo --version", "cargo metadata",
          'git commit -m "run npm install"', "rg 'cargo build'", 'echo "bun add left-pad"',
          "echo ok && depbox fetch -- bunx tool", "cargo test '", "depbox run -- nix develop -c cargo test --locked --offline"]
for provider in ("Bash", "functions.exec_command"):
    for command, alternative in scenarios:
        exercise(provider, command, "depbox " + alternative)
        exercise(provider, "depbox " + alternative)
    for command in passes:
        exercise(provider, command)
    exercise(provider, "bun install", "then depbox run -- bun run postinstall")
    exercise(provider, "bun add left-pad", overrides={"AGENT_NO_AUTHORING_HOOKS": "1"})
    exercise(provider, "bun add left-pad", overrides={"NORTH_AGENT_PYTHON": "/missing/python"})
    exercise(provider, raw="{bad json")
    exercise(provider, raw=json.dumps({"tool_name": provider, "tool_input": {"command": "bun add left-pad", "cmd": "bun add left-pad", "padding": "x" * 1048576}}))
    exercise(provider, raw=json.dumps({"tool_name": provider, "tool_input": []}))
    exercise(provider, "bun add left-pad", overrides={"NORTH_AGENT_ACTIVE": scratch + "/missing.active"})
    for state in ("hook another-guard\n", "skill dependency-exec-guard\n", "hook dependency-exec-guard\nhook dependency-exec-guard\n"):
        path = pathlib.Path(scratch) / "unavailable.active"
        path.write_text(state)
        exercise(provider, "bun add left-pad", overrides={"NORTH_AGENT_ACTIVE": str(path)})
    path = pathlib.Path(scratch) / "generation.json"
    unit = {"kind": "hook", "id": "dependency-exec-guard", "permission": "on", "active": True}
    generation = {"schema": "north.agent-activation/v1", "catalogDigest": "sha256:" + "0" * 64,
                  "generationId": "sha256:" + "1" * 64, "units": [unit]}
    path.write_text(json.dumps(generation))
    exercise(provider, "bun add left-pad", "depbox fetch -- bun add left-pad", overrides={"NORTH_AGENT_ACTIVATION": str(path), "NORTH_AGENT_ACTIVE": ""})
    for units in ([dict(unit, active=False)], [unit, unit]):
        generation["units"] = units
        path.write_text(json.dumps(generation))
        exercise(provider, "bun add left-pad", overrides={"NORTH_AGENT_ACTIVATION": str(path), "NORTH_AGENT_ACTIVE": ""})
    path.write_text("{invalid generation")
    exercise(provider, "bun add left-pad", overrides={"NORTH_AGENT_ACTIVATION": str(path), "NORTH_AGENT_ACTIVE": ""})
print(f"dependency-exec/provider boundary passed: {sum(counts.values())} checks; deny={counts['deny']} pass={counts['pass']}")
PY
