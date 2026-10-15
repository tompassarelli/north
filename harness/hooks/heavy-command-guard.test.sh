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
  AGENT_HOOK_ERRORS="$SCRATCH/errors.tsv" \
  python3 - "$HOOK" "$SCRATCH" <<'PY'
import json, os, pathlib, shlex, subprocess, sys, time

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

def exercise(provider, command, deny=False, override=False, cpu=False, **overrides):
    tool_input = {"command": command} if provider == "Bash" else {"cmd": command, "workdir": str(repo / "nested")}
    payload = {"tool_name": provider, "cwd": str(repo / "nested"), "tool_input": tool_input}
    if provider == "exec_command":
        payload["tool_input"] = json.dumps(tool_input)
        payload["cwd"] = scratch
    env = dict(os.environ)
    env.update(overrides)
    if override:
        env["AGENT_NO_AUTHORING_HOOKS"] = "1"
    start = time.monotonic()
    result = subprocess.run([hook], input=json.dumps(payload), text=True, capture_output=True, env=env, check=True)
    elapsed.append((time.monotonic() - start) * 1000)
    response = json.loads(result.stdout) if result.stdout else {}
    output = response.get("hookSpecificOutput", {})
    assert (output.get("permissionDecision") == "deny") == deny, (provider, command, result.stdout)
    if cpu and deny:
        assert output.get("permissionDecisionReason") == "only machine-capacity may throttle CPU; size the work with `machine-capacity run --class ...` or ask throughput-architect-lead", output
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

cpu_commands = []
for prop in ("CPUQuota=50%", "CPUQuotaPeriodSec=100ms", "CPUWeight=20", "StartupCPUWeight=20", "AllowedCPUs=0-1"):
    for user in ("", "--user "):
        cpu_commands.append(f"systemctl {user}set-property lead-x.scope {prop}")
for prop in ("CPUQuota=50%", "CPUWeight=20", "AllowedCPUs=0-1"):
    for option in (f"-p {prop}", f"-p{prop}", f"--property={prop}", f"--property {prop}"):
        cpu_commands.append(f"systemd-run --user {option} -- echo ready")
for leaf in ("cpu.max", "cpu.weight", "cpu.weight.nice"):
    for directory in ("/sys/fs/cgroup", "/sys/fs/cgroup/user.slice/lead-x.scope"):
        path = f"{directory}/{leaf}"
        cpu_commands.extend((f"echo 20>{path}", f"echo 20 >> '{path}'", f"tee {path}", f"printf 20 | tee -a {path}"))

for provider in ("Bash", "exec_command", "functions.exec_command"):
    for command in cpu_commands:
        for wrapped in (command, "echo ready && " + command, "sudo " + command, "bash -c " + shlex.quote(command)):
            exercise(provider, wrapped, True, cpu=True)
    for command in (
        "cat /sys/fs/cgroup/cpu.stat",
        "cat /sys/fs/cgroup/cpu.max /sys/fs/cgroup/cpu.weight /sys/fs/cgroup/cpu.weight.nice",
        "cat < /sys/fs/cgroup/cpu.max",
        "systemctl show -p CPUQuota lead-x.scope",
        "systemctl --user set-property lead-x.scope MemoryHigh=1G",
        "systemd-run -p MemoryHigh=1G -- echo ready",
        "systemd-run -- echo -p CPUQuota=50%",
        "tee /tmp/cpu.max",
        "echo 20 > /tmp/cpu.weight",
        "echo 'systemctl set-property lead-x.scope CPUQuota=50%'",
        "echo 'systemd-run -p CPUWeight=20 -- echo ready'",
        "echo '>/sys/fs/cgroup/cpu.max'",
        "echo '>' /sys/fs/cgroup/cpu.max",
        "echo ready # > /sys/fs/cgroup/cpu.max",
        "cat <<'BODY'\necho 20 > /sys/fs/cgroup/cpu.max\nBODY",
        "machine-capacity run --class moderate -- systemctl set-property lead-x.scope CPUQuota=50%",
        "bun /opt/machine-capacity.mjs run --class moderate -- systemd-run -p CPUWeight=20 -- echo ready",
        "node /opt/machine-capacity.mjs run --class moderate -- bash -c 'echo 20 > /sys/fs/cgroup/cpu.max'",
        "machine-capacity run --class moderate -- echo 20 > /sys/fs/cgroup/cpu.max",
        "bash -c 'machine-capacity run --class moderate -- tee /sys/fs/cgroup/cpu.max'",
    ):
        exercise(provider, command)
    exercise(provider, "echo machine-capacity > /sys/fs/cgroup/cpu.max", True, cpu=True)
    exercise(provider, cpu_commands[0], override=True)
    inactive = pathlib.Path(scratch) / "inactive.active"
    inactive.write_text("")
    exercise(provider, cpu_commands[0], NORTH_AGENT_ACTIVE=str(inactive))
    exercise(provider, cpu_commands[0], NORTH_AGENT_PYTHON=str(pathlib.Path(scratch) / "missing-python"))

for payload in ('{"tool_name":"Bash","systemctl":', json.dumps({"tool_name": "Bash", "tool_input": {"command": cpu_commands[0]}}) + " " * 1048576):
    result = subprocess.run([hook], input=payload, text=True, capture_output=True, check=True)
    assert not result.stdout and not result.stderr, result

marker = pathlib.Path(scratch) / "python-started"
sentinel = pathlib.Path(scratch) / "python-sentinel"
sentinel.write_text("#!/usr/bin/env bash\nprintf started > " + shlex.quote(str(marker)) + "\n")
sentinel.chmod(0o755)
payload = {"tool_name": "Bash", "cwd": scratch, "tool_input": {"command": "printf ready"}}
subprocess.run([hook], input=json.dumps(payload), text=True, capture_output=True,
               env=dict(os.environ, NORTH_AGENT_PYTHON=str(sentinel)), check=True)
assert not marker.exists(), "ordinary commands started Python"
print("CPU throttles denied across providers, wrappers, writes; reads, helper, overrides and fail-open passed")
PY
