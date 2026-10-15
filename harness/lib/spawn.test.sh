#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/agents-spawn-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT
export HOME="$scratch/home" XDG_STATE_HOME="$scratch/state" AGENTS_ORG_FILE="$scratch/org.json"
export AGENTS_ORCHESTRATION="$scratch/orchestration.toml" AGENTS_SPAWN_DRY=1
export AGENT_ORG_NAME=parent CLAUDE_CODE_SESSION_ID=parent-session AGENTS_SESSION_HOST=tmux
unset CODEX_THREAD_ID CLAUDE_CONFIG_DIR
mkdir -p "$HOME/claude-c"
printf '{}\n' >"$HOME/claude-c/.credentials.json"
printf '{"oauthAccount":{"accountUuid":"fixture-c"}}\n' >"$HOME/claude-c/.claude.json"
python3 - "$AGENTS_ORCHESTRATION" "$HOME" <<'PY'
import hashlib
from pathlib import Path
import socket
import sys
path, home = sys.argv[1:]
Path(path).write_text(f'''
[accounts.claude.labels]
{hashlib.sha256(b"fixture-c").hexdigest()[:8]} = "c"
[accounts.claude.profiles.c.{socket.gethostname()}]
path = "{home}/claude-c"
[tiers."claude:opus-medium"]
agent = "opus-medium"
model = "claude-opus-5-5"
effort = "medium"
start = 1
''')
PY
"$repo/bin/agents" org add parent --role lead --domain north --session parent-session >/dev/null
cat >"$scratch/run.py" <<'PY'
import json
import os
from pathlib import Path
import sys
sys.path.insert(0, str(Path(sys.argv[1]) / "harness/lib"))
import spawn
spawn.placement.place_spawn = lambda *args: json.loads(os.environ["DECISION"])
sys.exit(spawn.main(sys.argv[2:]))
PY
printf 'Item: north#spawn\nCategory: tooling\nDo the work.\n' >"$scratch/brief.md"
export DECISION='{"kind":"launch","provider":"claude","account":"claude:c","tier":"claude:opus-medium","model":"claude-opus-5-5","effort":"medium"}'
python3 "$scratch/run.py" "$repo" "$scratch/brief.md" --tier opus-medium >"$scratch/claude.out"
python3 "$scratch/run.py" "$repo" "$scratch/brief.md" --tier opus-medium >"$scratch/retry.out"
id=$(sha256sum < <(printf 'parent-session'; cat "$scratch/brief.md"))
id=${id:0:12}
python3 "$scratch/run.py" "$repo" "$id" >"$scratch/id-retry.out"
export DECISION='{"kind":"launch","provider":"codex","account":"codex","tier":"codex:sol-high","model":"gpt-6.1-sol","effort":"high"}'
printf 'Item: north#codex\nDo the work.\n' >"$scratch/codex.md"
python3 "$scratch/run.py" "$repo" "$scratch/codex.md" >"$scratch/codex.out"
export DECISION='{"kind":"native"}'
printf 'Item: north#native\nDo the work.\n' >"$scratch/native.md"
python3 "$scratch/run.py" "$repo" "$scratch/native.md" --tier opus-medium >"$scratch/native.out"
export DECISION='{"kind":"queued","reason":"short-window"}'
set +e
python3 "$scratch/run.py" "$repo" "$scratch/native.md" >"$scratch/queued.out"
code=$?
set -e
[ "$code" = 3 ]
python3 - "$scratch" "$id" <<'PY'
import hashlib
import json
import os
from pathlib import Path
import shlex
import sys
root, ident = Path(sys.argv[1]), sys.argv[2]
claude = shlex.split((root / "claude.out").read_text().splitlines()[0])
assert f"CLAUDE_CONFIG_DIR={root}/home/claude-c" in claude, claude
assert "AGENT_ROLE=worker" in claude and "AGENT_DELEGATION_BUDGET=0" in claude, claude
assert claude[claude.index("--model") + 1] == "claude-opus-5-5", claude
assert claude[claude.index("--effort") + 1] == "medium", claude
nodes = json.loads((root / "org.json").read_text())["nodes"]
node = next(n for n in nodes if n["id"] == f"spawn-{ident}")
assert node["parent"] == "parent" and node["parent_session"] == "parent-session", node
assert node["role"] == "worker" and node["budget"] == 0, node
brief = Path(node["brief"]).read_text()
assert f"agents event LAND-READY|STUCK 'north#spawn' --from-node spawn-{ident} --to-node parent --body <report>" in brief, brief
assert brief.endswith(f"agents org finish spawn-{ident}\n"), brief
for retry in ("retry.out", "id-retry.out"):
    assert (root / retry).read_text().count("already active") == 1, retry
    assert "env " not in (root / retry).read_text(), retry
codex = shlex.split((root / "codex.out").read_text().splitlines()[0])
assert codex[codex.index("codex-lead") + 1] == "start", codex
assert codex[codex.index("-m") + 1] == "gpt-6.1-sol", codex
assert codex[codex.index("-c") + 1] == "model_reasoning_effort=high", codex
assert "--org-name" in codex, codex
native = (root / "native.out").read_text()
assert native.startswith('Agent(subagent_type="opus-medium", prompt='), native
assert len(nodes) == 3, nodes
assert (root / "queued.out").read_text() == "queued: short-window\n"
print("ok: placed Claude and Codex workers keep their account, tier, parent and return footer")
print("ok: retry by brief or id launches nothing; native returns the Agent call; queued exits 3")
PY
cat "$scratch/claude.out" "$scratch/codex.out" "$scratch/native.out" "$scratch/retry.out"
python3 - "$repo" "$scratch" <<'PY'
import hashlib
import json
from pathlib import Path
import sys
sys.path.insert(0, str(Path(sys.argv[1]) / "harness/lib"))
import spawn
root = Path(sys.argv[2])
brief = root / "failed-launch.md"
brief.write_text("Item: north#failed-launch\nDo the work.\n")
ident = hashlib.sha256(b"parent-session" + brief.read_bytes()).hexdigest()[:12]
name = f"spawn-{ident}"
spawn.placement.place_spawn = lambda *args: {
    "kind": "launch", "provider": "claude", "account": "claude:c",
    "tier": "claude:opus-medium", "model": "claude-opus-5-5", "effort": "medium"}
launches = []
def launch(node, *args, **kwargs):
    launches.append(node["id"])
    assert spawn.main([ident]) == 0
    return None if len(launches) == 1 else "retry-session"
spawn.org.launch = launch
try:
    spawn.main([str(brief)])
except SystemExit as error:
    assert error.code == 1, error.code
else:
    raise AssertionError("failed launch must exit 1")
with spawn.org.Org() as registry:
    node = registry.need(name)
    assert node.get("died") and not node["session"], node
assert spawn.main([ident]) == 0
assert launches == [name, name], launches
with spawn.org.Org() as registry:
    node = registry.need(name)
    assert node["session"] == "retry-session" and not node.get("died"), node
assert spawn.main([str(brief)]) == 0
assert launches == [name, name], launches
print("ok: failed launch retries once and records its session; starting and live retries launch nothing")
PY
python3 - "$repo" "$scratch" <<'PY'
from contextlib import redirect_stdout
import hashlib
import io
from pathlib import Path
import sys
sys.path.insert(0, str(Path(sys.argv[1]) / "harness/lib"))
import spawn
root = Path(sys.argv[2])
brief = root / "missing-profile.md"
brief.write_text("Item: north#missing-profile\nDo the work.\n")
ident = hashlib.sha256(b"parent-session" + brief.read_bytes()).hexdigest()[:12]
name = f"spawn-{ident}"
spawn.placement.place_spawn = lambda *args: {
    "kind": "launch", "provider": "claude", "account": "claude:c",
    "tier": "claude:opus-medium", "model": "claude-opus-5-5", "effort": "medium"}
profile = root / "home/claude-c"
missing = root / "home/claude-c-missing"
profile.rename(missing)
try:
    spawn.main([str(brief)])
except SystemExit as error:
    assert error.code == 1, error.code
else:
    raise AssertionError("missing profile must fail the launch")
finally:
    missing.rename(profile)
with spawn.org.Org() as registry:
    node = registry.need(name)
    assert node.get("died") and not node["session"], node
launch = spawn.org.launch
def in_progress(node, *args, **kwargs):
    assert spawn.main([ident]) == 0
    return launch(node, *args, **kwargs)
spawn.org.launch = in_progress
out = io.StringIO()
with redirect_stdout(out):
    assert spawn.main([ident]) == 0
    assert spawn.main([str(brief)]) == 0
assert out.getvalue().count("env ") == 1, out.getvalue()
assert out.getvalue().count("already active") == 2, out.getvalue()
with spawn.org.Org() as registry:
    node = registry.need(name)
    assert node["session"] and not node.get("died"), node
print("ok: restoring a missing profile launches exactly once; in-progress and active retries stay suppressed")
PY
python3 - "$repo" "$scratch" <<'PY'
from contextlib import redirect_stdout
import io
import json
import os
from pathlib import Path
import sys
sys.path.insert(0, str(Path(sys.argv[1]) / "harness/lib"))
import spawn
root = Path(sys.argv[2])
os.environ["XDG_STATE_HOME"] = str(root / "release-state")
state = spawn.pace_sampler.state_dir()
state.mkdir(parents=True)
gate = state / "usage-gate.json"
gate.write_text(json.dumps({"claude:c": {"held": True}}))
workspace = root / "workspace"
workspace.mkdir()
os.chdir(workspace)
def place(text, spawner, gate, cfg, now):
    assert spawner["parent"] == "parent" and spawner["session"] == "parent-session", spawner
    if gate["claude:c"]["held"]:
        return {"kind": "queued", "account": "claude:c", "reason": "workers", "eta": "2026-10-20T06:00:00Z"}
    return {"kind": "native", "provider": "claude", "account": "claude:c",
            "tier": "claude:opus-medium", "model": "claude-opus-5-5", "effort": "medium",
            "host": "unavailable-host" if "Host: unavailable" in text else spawn.resident_cap.host()}
spawn.placement.place_spawn = place
briefs = []
for name in ("older", "newer"):
    brief = root / f"{name}.md"
    brief.write_text(f"Item: north#{name}\nDo the work.\n")
    briefs.append(brief)
    assert spawn.main([str(brief), "--tier", "opus-medium"]) == 3
jobs = spawn.spawn_queue.read(state, 0)
assert len(jobs) == 2, jobs
original = jobs[0]
assert spawn.main([str(briefs[0]), "--tier", "opus-medium"]) == 3
jobs = spawn.spawn_queue.read(state, 0)
assert len(jobs) == 2 and jobs[0]["id"] == original["id"], jobs
assert jobs[0]["cwd"] == str(workspace), jobs
assert all(key in jobs[0] for key in ("spawn_id", "parent", "session", "account", "tier", "cwd")), jobs
assert len(spawn.spawn_queue.read(state, 99999999999)) == 2
print("ok: duplicate spawn ids keep one durable queued job with the original launch context")
gate.write_text(json.dumps({"claude:c": {"held": False}}))
third = root / "third.md"
third.write_text("Item: north#third\nDo the work.\n")
assert spawn.main([str(third), "--tier", "opus-medium"]) == 3
assert len(spawn.spawn_queue.read(state, 0)) == 3
os.environ.update(AGENT_ORG_NAME="sampler", CLAUDE_CODE_SESSION_ID="sampler-session")
os.chdir(root)
out = io.StringIO()
with redirect_stdout(out):
    assert spawn.main(["release"]) == 0
assert "env " in out.getvalue() and "Agent(" not in out.getvalue(), out.getvalue()
with spawn.org.Org() as registry:
    first = registry.need(f"spawn-{original['spawn_id']}")
    assert first["cwd"] == str(workspace) and first["parent_session"] == "parent-session", first
    assert first["role"] == "worker" and first["budget"] == 0, first
jobs = spawn.spawn_queue.read(state, 0)
assert len(jobs) == 2 and jobs[0]["spawn_id"] != original["spawn_id"], jobs
print("ok: one release launches the oldest eligible job as a durable worker and leaves newer jobs queued")
(state / "spawn" / f"{jobs[0]['spawn_id']}.md").write_text("Item: north#newer\nHost: unavailable\n")
out = io.StringIO()
with redirect_stdout(out):
    assert spawn.main(["release"]) == 0
assert out.getvalue().count("env ") == 1, out.getvalue()
assert [job["spawn_id"] for job in spawn.spawn_queue.read(state, 0)] == [jobs[0]["spawn_id"]]
print("ok: an older unavailable host leaves capacity free for a newer eligible job")
spawn.spawn_queue.enqueue_spawn(state, original, {"reason": "crash before queue removal", "account": "claude:c"})
out = io.StringIO()
with redirect_stdout(out):
    assert spawn.main(["release"]) == 0
assert "env " not in out.getvalue(), out.getvalue()
assert [job["spawn_id"] for job in spawn.spawn_queue.read(state, 0)] == [jobs[0]["spawn_id"]]
print("ok: release reconciles an active org node after a launch crash and never launches it twice")
import signal
spawn.placement.place_spawn = lambda *args: signal.raise_signal(signal.SIGALRM)
assert spawn.release() == 2
print("ok: a release deadline returns control with the queued job retained")
spawn.placement.place_spawn = lambda *args: {
    "kind": "launch", "provider": "claude", "account": "claude:c",
    "tier": "claude:opus-medium", "model": "claude-opus-5-5", "effort": "medium"}
spawn.pace_sampler.usage.DB = str(root / "sampler-usage.db")
spawn.pace_sampler.usage.REMOTE = False
out = io.StringIO()
with redirect_stdout(out):
    spawn.pace_sampler.sample("fixture-no-adapters", notify=False)
assert out.getvalue().count("env ") == 1, out.getvalue()
assert spawn.spawn_queue.read(state, 0) == []
print("ok: a pace sampler pass automatically releases a queued durable worker")
PY
