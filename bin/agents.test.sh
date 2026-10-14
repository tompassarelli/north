#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
agents="$repo/bin/agents"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/agents-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT

if AGENTS_NORTH_BIN="$scratch/bin/missing" "$agents" status \
  >"$scratch/missing.out" 2>"$scratch/missing.err"; then
  printf 'agents accepted a missing North CLI\n' >&2
  exit 1
fi

printf 'ok: agents is a thin North CLI client\n'

export AGENTS_ORCHESTRATION="$scratch/orchestration.toml" THREADS_DB="$scratch/snapshot.db"
cat >"$AGENTS_ORCHESTRATION" <<'TOML'
[projects.north]
target = "2026-10-13T09:30:00Z"
[projects.wisp]
TOML

export AGENTS_ORG_FILE="$scratch/proxy-org.json"
"$agents" org set proxy --provider codex --session 01a13b4d-b6d0-7221-8f25-0fa0e99d558d \
  --status "$scratch/proxy-inbox.md" --pid "$$" >"$scratch/proxy-register.out"
"$agents" org set proxy --pid "$$" >"$scratch/proxy-pid.out"
"$agents" org show >"$scratch/proxy-show.out"
python3 - "$AGENTS_ORG_FILE" "$scratch" <<'PROXY'
import json, sys
from pathlib import Path
path, scratch = map(Path, sys.argv[1:])
proxy = json.loads(path.read_text())["proxy"]
assert proxy["provider"] == "codex", proxy
assert proxy["session"] == "01a13b4d-b6d0-7221-8f25-0fa0e99d558d", proxy
assert proxy["status_file"] == str(scratch / "proxy-inbox.md"), proxy
assert "codex" in (scratch / "proxy-pid.out").read_text()
assert "codex:01a13b4d-b6d0-7221-8f25-0fa0e99d558d" in (scratch / "proxy-show.out").read_text()
PROXY
cp "$AGENTS_ORG_FILE" "$scratch/proxy-before.json"
if "$agents" org set proxy --provider unknown --pid "$$" >"$scratch/proxy-refuse.out" 2>&1; then
  printf 'proxy accepted an unknown provider\n' >&2
  exit 1
fi
cmp "$AGENTS_ORG_FILE" "$scratch/proxy-before.json"
unset AGENTS_ORG_FILE
printf 'ok: explicit Codex proxy registration survives PID-only updates\n'

python3 - "$THREADS_DB" <<'PYTHON'
import sqlite3
import sys
with sqlite3.connect(sys.argv[1]) as db:
    db.execute("CREATE TABLE runs(agent TEXT, item TEXT, outcome TEXT, role TEXT, staffed TEXT, started TEXT, ended TEXT, landed_at TEXT, cost_usd REAL, expects_landing INTEGER, landed INTEGER)")
    db.executemany("INSERT INTO runs VALUES (?, ?, 'done', 'worker', ?, ?, ?, ?, ?, 1, 1)", [
        (f"{project}-{number}", f"{project}#{number}", "2026-10-12T06:00:00Z", "2026-10-12T06:00:00Z", end, end, cost)
        for project in ("north", "wisp") for number, end, cost in ((1, "2026-10-12T06:20:00Z", 1), (2, "2026-10-12T06:40:00Z", 3))])
PYTHON
mkdir -p "$scratch/home/.local/state/agents"
printf '{"committedBatchCpus": 0, "aggregateCpuLimit": 20, "protectedCpuSomeAvg10": 0}\n' >"$scratch/probe.json"
printf '{"claude": {"epoch": 9999999999, "stale_min": 30, "refusal": null, "paces": {}}, "codex": {"epoch": 9999999999, "stale_min": 30, "refusal": null, "paces": {}}}\n' >"$scratch/home/.local/state/agents/usage-gate.json"
quote() {
  HOME="$scratch/home" XDG_STATE_HOME="$scratch/home/.local/state" SPAWN_CAPACITY_STATUS="$scratch/probe.json" \
    SPAWN_CAPACITY_HOME="$scratch/home" "$agents" plan --quote --project "$1" --boxes 4
}
quote north >"$scratch/dated.out"
quote wisp >"$scratch/undated.out"
python3 - "$scratch" <<'PYTHON'
from datetime import datetime, timezone
from pathlib import Path
import re
import sys
scratch = Path(sys.argv[1])
dated = (scratch / "dated.out").read_text().splitlines()
undated = (scratch / "undated.out").read_text().splitlines()
decisions = [line for line in dated if line.startswith("Blocked: decision for Tom:")]
assert len(decisions) == 1, dated
assert not any(line.startswith("Blocked: decision for Tom:") for line in undated), undated
match = re.fullmatch(r"Blocked: decision for Tom: north date (.+) \$8.00 \[approve\|later date\|smaller scope\]", decisions[0])
assert match, decisions
minutes = (datetime.fromisoformat(match[1].replace("Z", "+00:00")) - datetime.now(timezone.utc)).total_seconds() / 60
assert 29 <= minutes <= 30, minutes
assert "minutes/box 30" in dated[0] and "width 4" in dated[0] and "concurrency 2" in dated[0], dated
PYTHON
printf 'ok: project quote takes the critical path at the gate width from snapshot measurements, with one dated decision\n'

git_fixture() {
  git -C "$1" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "$2"
}
mkdir -p "$scratch/bin" && printf '#!/bin/sh\necho published\n' >"$scratch/bin/north" && chmod +x "$scratch/bin/north"
for case in behind diverged current offline; do
  root="$scratch/$case"
  for repo in south north; do
    git init -q -b main "$root/$repo-origin" && git_fixture "$root/$repo-origin" base
    git clone -q "$root/$repo-origin" "$root/$repo"
  done
  mkdir -p "$root/south/agents" && touch "$root/south/untracked"
  [ "$case" = current ] || git_fixture "$root/south-origin" remote
  [ "$case" = diverged ] && git_fixture "$root/south" local
  if [ "$case" = offline ]; then
    git -C "$root/south" remote set-url origin "$root/gone"
    printf '{"activation": {}, "registrations": {}, "baselines": []}\n' >"$root/south/agents/catalog-config.json"
    printf '[projects]\n' >"$root/south/agents/orchestration.toml"
    printf '# Policy\n' >"$root/south/agents/AGENTS.md"
    sha=$(sha256sum "$root/south/agents/AGENTS.md" | cut -d' ' -f1)
    printf 'date = "d"\nmodel = "m"\npolicy_sha256 = "%s"\nstartup_tokens = 1000000\nconformance = 1.0\nn = 1\n' "$sha" >"$root/south/agents/operating-point.toml"
    mkdir -p "$root/replay" && printf '{"policy_sha256": "%s", "model": "m", "conformance": 1.0, "n": 1}\n' "$sha" >"$root/replay/sweep-d.jsonl"
  fi
  before=$(git -C "$root/south" rev-parse HEAD)
  AGENTS_NORTH_BIN="$scratch/bin/north" AGENTS_OPERATOR_DIR="$root/south/agents" AGENTS_REPLAY_DIR="$root/replay" \
    NORTH_REPO_ROOTS="{\"north\":\"$root/north\",\"nixos-config\":\"$root/none\"}" \
    "$agents" sync >"$root/out" 2>&1 || true
  after=$(git -C "$root/south" rev-parse HEAD)
  origin=$(git -C "$root/south-origin" rev-parse main)
  case "$case" in
    behind) [ "$after" = "$origin" ] || { printf 'sync did not fast-forward a clean behind main\n' >&2; exit 1; } ;;
    diverged)
      grep -q '^south main is behind origin by 1 and not fast-forwardable: diverged' "$root/out" && [ "$after" = "$before" ] \
        || { printf 'sync did not refuse a diverged main\n' >&2; cat "$root/out" >&2; exit 1; } ;;
    offline)
      grep -q '^agents sync: south fetch failed; using local main' "$root/out" && grep -q '^published' "$root/out" && [ "$after" = "$before" ] \
        || { printf 'sync did not continue past a failed fetch\n' >&2; cat "$root/out" >&2; exit 1; } ;;
    current) [ "$after" = "$before" ] && ! grep -q 'behind origin' "$root/out" \
        || { printf 'sync changed a current main\n' >&2; exit 1; } ;;
  esac
done
printf 'ok: sync fast-forwards clean behind mains, refuses diverged ones, leaves current ones and publishes from local main when fetch fails\n'
