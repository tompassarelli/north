#!/usr/bin/env bash
# Provider-neutral anti-rot check for the shared agent harness and its adapters.
# Default output is intentionally a small status report. Use --verbose for the
# individual assertions; failures always print their full diagnostic.
set -uo pipefail

if [ -n "${NORTH_AGENT_PYTHON:-}" ]; then
  AGENT_CONFIG_PYTHON="$NORTH_AGENT_PYTHON"
elif command -v python3 >/dev/null 2>&1; then
  AGENT_CONFIG_PYTHON="$(command -v python3)"
else
  AGENT_CONFIG_PYTHON=/etc/codex/hooks/runtime/python3
fi

codex_managed_policy_binding_count() {
  "$AGENT_CONFIG_PYTHON" - "$1" "$OPERATOR" <<'PY'
import sys
import tomllib

with open(sys.argv[1], "rb") as handle:
    policy = tomllib.load(handle)
print(sum(
    len(binding["hooks"])
    for event, bindings in policy.get("hooks", {}).items()
    if event != "managed_dir"
    for binding in bindings
))
PY
}

canonical_link() {
  local link="$1" expected="$2" label="$3"
  local got want
  got="$(readlink -f "$link" 2>/dev/null || true)"
  want="$(readlink -f "$expected" 2>/dev/null || true)"
  if [ -n "$got" ] && [ "$got" = "$want" ]; then ok_detail "$label → ${want#"$FIRN"/}"
  else bad "$label resolves to '${got:-missing}', expected '$want'"; fi
}

immutable_store_link_matches() {
  local link="$1" expected="$2" label="$3" resolved
  local store_prefix="${NIX_STORE_PREFIX:-/nix/store}"

  if [ ! -L "$link" ]; then
    bad "$label must be a generation-owned symlink"
    return 1
  fi
  resolved="$(readlink -f "$link" 2>/dev/null || true)"
  case "$resolved" in
    "$store_prefix"/*) ;;
    *)
      bad "$label resolves outside $store_prefix: ${resolved:-missing}"
      return 1
      ;;
  esac
  if cmp -s "$resolved" "$expected"; then
    ok_detail "$label is an exact generation-owned store copy"
  else
    bad "$label differs from the committed generation source"
    return 1
  fi
}

run_agent_policy_contract() {
  local repo="$1" local_mode="${2:-0}"
  local -a args=(--repo "$repo")
  [ "$local_mode" -eq 0 ] || args+=(--local)
  north agent-policy-contract --north "$NORTH" "${args[@]}"
}

policy_gate() {
  AGENT_CONFIG_HOOKS="$(dirname "${BASH_SOURCE[0]}")/../hooks" AGENT_CONFIG_SCRIPTS="$(dirname "${BASH_SOURCE[0]}")" \
    "$AGENT_CONFIG_PYTHON" - "$1" "$OPERATOR" "${2:-}" <<'PY'
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tomllib

sys.path.insert(0, os.environ["AGENT_CONFIG_SCRIPTS"])
from policy_stats import compare, covered, flips, wilson

PLAYBOOK_LINES, SWEEP_DUE_SHARE = 500, 0.15
repo = pathlib.Path(sys.argv[1])
OPERATOR = pathlib.Path(sys.argv[2])
MODE = sys.argv[3]
HOME = pathlib.Path.home()
REPLAY = pathlib.Path(os.environ.get("AGENTS_REPLAY_DIR") or HOME / ".local/state/agents/replay")


ROOTS = json.loads(os.environ.get("NORTH_REPO_ROOTS") or "null") or {}


def owner_path(unit):
    owner = catalog["registrations"].get(unit, {}).get("owner")
    if owner:
        name = owner.get("repo")
        root = repo if name in (None, "nixos-config", "firn") else pathlib.Path(
            ROOTS.get(name) or pathlib.Path.home() / "code" / name / "main")
        return root / owner["path"]
    try:
        found = subprocess.run(["agents", "path", unit], capture_output=True, text=True)
    except OSError:
        return None
    return pathlib.Path(found.stdout.strip()) if found.returncode == 0 else None


catalog = json.loads((OPERATOR / "catalog-config.json").read_text())
errors, missing, total, listing, claude_listing = [], [], 0, 0, 0
for unit, entry in catalog["activation"].items():
    if not any(
        d.get("type") == "skill" and "shared" in d.get("targets", [])
        for d in entry.get("distributions", [])
    ):
        continue
    skill = owner_path(unit)
    if not skill or not skill.is_file():
        missing.append(unit)
        continue
    match = re.match(r"---\n(.*?)\n---\n", skill.read_text(), re.S)
    if not match:
        errors.append(f"{unit}: SKILL.md has no frontmatter")
        continue
    front = match.group(1) + "\n"
    lines = len(skill.read_text()[match.end():].splitlines())
    total += lines
    if lines > PLAYBOOK_LINES and not re.search(r"^metadata:\n(?:[ \t]+.*\n)*?[ \t]+kind:[ \t]*domain[ \t]*$", front, re.M):
        errors.append(
            f"{unit}: playbook body is {lines} lines; policy keeps each playbook body at "
            f"most {PLAYBOOK_LINES} (set metadata.kind: domain for a domain skill)"
        )
    agents = re.search(r"^agents:\s*\[(.*)\]", front, re.M)
    description = re.search(r"^description:[ \t]*(?:[>|]-?)?[ \t]*\n?((?:.*\n)*?)(?=^\S|\Z)", front, re.M)
    entry = len(unit) + len(" ".join(description.group(1).split()) if description else "")
    if not agents or "claude" in agents.group(1):
        claude_listing += entry + 4
    if agents and "codex" not in agents.group(1):
        continue
    listing += entry
summary = f"{total} SKILL body lines · {listing} Codex listing chars"
if missing:
    summary += f" · {len(missing)} unreadable sources skipped"


def instructions(target):
    text = ""
    for baseline in catalog.get("baselines", []):
        if target in baseline.get("targets", []):
            owner = baseline["owner"]
            name = owner.get("repo")
            root = OPERATOR.parent if name == "south" and owner["path"].startswith("agents/") else pathlib.Path(
                ROOTS.get(name) or HOME / "code" / name / "main")
            text += f"<!-- {name}:{owner['path']} -->\n" + (root / owner["path"]).read_text() + "\n\n"
    return len(text)


def session_start(fixture):
    hooks = pathlib.Path(os.environ["AGENT_CONFIG_HOOKS"])
    payload = json.dumps({"hook_event_name": "SessionStart", "source": "startup", "session_id": "budget", "cwd": str(HOME)})
    worst = (0, "")
    env = {k: v for k, v in os.environ.items() if k != "AGENT_NO_AUTHORING_HOOKS"}
    env["CLAUDE_CODE_ENTRYPOINT"] = "cli"
    if fixture:
        env["LANE_GC_UNLANDED"] = os.devnull
    size = len(subprocess.run(["bash", str(hooks / "unlanded-work.sh")], input=payload, env=env,
                              capture_output=True, text=True, timeout=10).stdout)
    return size, "fleet"


def startup_tokens():
    projects = tomllib.loads((OPERATOR / "orchestration.toml").read_text()).get("projects", {})
    repos = [(len(path.read_text()), name) for name in projects
             for path in [pathlib.Path(ROOTS.get(name) or HOME / "code" / name / "main") / "AGENTS.md"] if path.is_file()]
    agent_chars = 0
    for path in (HOME / ".claude/agents").glob("*.md"):
        front = re.match(r"---\n(.*?)\n---\n", pathlib.Path(path).read_text(), re.S)
        for field in ("name", "description"):
            value = re.search(rf"^{field}:\s*(.*)$", front.group(1), re.M) if front else None
            agent_chars += len(value.group(1)) if value else 0
    start_chars, start_role = session_start(True)
    live_chars, _ = session_start(False)
    parts = {"policy": instructions("shared"), "code": instructions("code"),
             "skills": claude_listing, "agents": agent_chars, f"SessionStart {start_role}": start_chars}
    detail = ", ".join(f"{name} {chars // 4}" for name, chars in parts.items())
    project = ", ".join(f"{name} {chars // 4}" for chars, name in sorted(repos, reverse=True)) or "none"
    return sum(parts.values()) // 4, (f"chars/4: {detail} (fixture state; live SessionStart "
                                      f"{live_chars // 4}, not gated); harness-bundled listing not counted; "
                                      f"project AGENTS.md, not gated: {project}")


def sweep_row(sha, wanted=None):
    newest = full = None
    for path in sorted([*(OPERATOR / "replay").glob("sweep-*.jsonl"), *REPLAY.glob("sweep-*.jsonl")], key=lambda path: path.name):
        for line in path.read_text().splitlines():
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(row, dict) and row.get("policy_sha256") == sha:
                newest = row
                scenarios = row.get("scenarios") or {}
                need = scenarios.keys() if wanted is None else wanted
                if need and covered(scenarios, need) == len(need):
                    full = row
    return full or newest


def scenario_ids():
    ids = set()
    for path in (OPERATOR / "replay").glob("*.json"):
        try:
            ids.add(json.loads(path.read_text()).get("id") or path.stem)
        except (json.JSONDecodeError, AttributeError):
            ids.add(path.stem)
    return ids


def regression(point, sha, tokens):
    if point is None:
        return "agents sync: no operating point; run the sweep and promote one", None
    old_c, old_n = float(point["conformance"]), int(point["n"])
    point_row = sweep_row(point.get("policy_sha256")) or {}
    wanted = scenario_ids()
    if point_row.get("scenarios"):
        wanted &= set(point_row["scenarios"])
    row = sweep_row(sha, wanted)
    if row is None:
        return (f"agents sync: the live policy has no sweep row in {OPERATOR / 'replay'} or {REPLAY}; "
                "run policy-replay.py --sweep with it as a variant"), None
    if row.get("model") != point.get("model"):
        return (f"agents sync: the live policy's sweep row ran on {row.get('model')}, "
                f"the operating point on {point.get('model')}; rerun the sweep on one model"), None
    count = covered(row.get("scenarios") or {}, wanted)
    if count < len(wanted):
        return f"agents sync: sweep row for {sha[:8]} covers {count} of {len(wanted)} scenarios", None
    flipped = flips(point_row.get("scenarios") or {}, row.get("scenarios") or {})
    if flipped:
        return (f"agents sync: scenario {', '.join(flipped)} flipped to 0 passes where the operating point "
                "passed every trial; confirm at 10 trials per arm"), None
    new_c, new_n = float(row["conformance"]), int(row["n"])
    verdict = compare((old_c, old_n), (new_c, new_n))
    if verdict == "drop":
        return f"agents sync: replay conformance dropped {old_c:g}→{new_c:g}", None
    if tokens > int(point["startup_tokens"]) and verdict != "gain":
        return (f"agents sync: startup tokens grew {point['startup_tokens']}→{tokens} with no measured "
                "conformance gain; run policy-replay.py --sweep"), None
    lo, hi = wilson(old_c, old_n)
    return None, (f"session start {tokens} tokens (operating point {point.get('variant', '?')} {point['startup_tokens']} "
                  f"at {point['date']}, conformance {old_c:g} [{lo:.3f}, {hi:.3f}] n={old_n})")


if MODE == "measure":
    tokens, detail = startup_tokens()
    print(f"session start {tokens} tokens")
    print(detail)
elif MODE == "session":
    tokens, detail = startup_tokens()
    sha = hashlib.sha256((OPERATOR / "AGENTS.md").read_bytes()).hexdigest()
    point_file = OPERATOR / "operating-point.toml"
    point = tomllib.loads(point_file.read_text()) if point_file.is_file() else None
    failure, line = regression(point, sha, tokens)
    if failure:
        errors.append(failure)
    else:
        print(line)
    print(detail)
    if point and abs(tokens - int(point["startup_tokens"])) > SWEEP_DUE_SHARE * int(point["startup_tokens"]):
        print(f"sweep due: startup tokens {tokens} differ from the operating point's {point['startup_tokens']} by more than 15%")
print("\n".join(errors) if errors else summary)
sys.exit(1 if errors else 0)
PY
}

rule_register() {
  "$AGENT_CONFIG_PYTHON" - "$1" <<'PY'
import json
import os
import pathlib
import re
import subprocess
import sys
import tomllib

operator = pathlib.Path(sys.argv[1])
bullets, section = [], None
for line in (operator / "AGENTS.md").read_text().splitlines():
    if line.startswith("## "):
        section = line[3:]
    elif line.startswith("- "):
        bullets.append((section, line[2:]))
try:
    rules = tomllib.loads((operator / "rules.toml").read_text()).get("rule", [])
except (OSError, tomllib.TOMLDecodeError) as error:
    print(f"rules.toml is unreadable: {error}")
    sys.exit(1)
ids = [rule.get("id") for rule in rules]
placement = re.compile(r"global|dropped|(merged|skill|hook):[a-z0-9-]+")
errors = [f"rules.toml {rule.get('id')}: placement must be global, merged:<id>, skill:<name>, hook:<name> or dropped"
          for rule in rules if not placement.fullmatch(str(rule.get("placement", "")))]
errors += [f"rules.toml {rule.get('id')}: {rule['placement']} names no rules.toml id" for rule in rules
           if str(rule.get("placement", "")).startswith("merged:") and rule["placement"][7:] not in ids]
registered = [(rule.get("section"), rule.get("text")) for rule in rules if rule.get("placement") == "global"]
errors += [f"AGENTS.md bullet has no rules.toml entry: [{s}] {t}" for s, t in bullets if (s, t) not in registered]
errors += [f"rules.toml entry has no AGENTS.md bullet: [{s}] {t}" for s, t in registered if (s, t) not in bullets]
if not errors and registered != bullets:
    errors.append("rules.toml entries are not in AGENTS.md order")
if len(set(ids)) != len(ids) or not all(ids):
    errors.append("rules.toml ids must be present and unique")
errors += [f"rules.toml {rule.get('id')}: fixes is empty" for rule in rules if not str(rule.get("fixes", "")).strip()]
ledger = operator / "incidents.jsonl"
if ledger.is_file():
    roots = json.loads(os.environ.get("NORTH_REPO_ROOTS") or "{}") or {}

    def session(entry):
        ref = str(entry.get("ref") or "")
        issue = re.search(r"([\w.-]+)#(\d+)$", ref)
        if entry.get("session") or entry.get("source") not in ("git", "github"):
            return entry.get("session") or ref
        if entry.get("source") == "github" and issue:
            return f"{issue[1]}#{issue[2]}"
        repo, _, sha = ref.partition("@")
        root = roots.get(repo) or str(pathlib.Path.home() / "code" / repo / "main")
        shown = subprocess.run(["git", "-C", root, "show", "-s", "--format=%ae %as%n%B", sha],
                               capture_output=True, text=True)
        head, _, body = shown.stdout.partition("\n")
        cited = re.search(r"(?<![\w.-])(?:[\w.-]+/)?([\w.-]*)#(\d+)\b", body)
        if shown.returncode == 0 and cited:
            return f"{cited[1] or repo}#{cited[2]}"
        return f"{repo} {head}" if shown.returncode == 0 else f"{repo} {str(entry.get('ts'))[:10]}"

    sessions = {entry.get("id"): session(entry)
                for entry in map(json.loads, filter(str.strip, ledger.read_text().splitlines()))}
    git = ["git", "-C", str(operator)]
    prefix = subprocess.run(git + ["rev-parse", "--show-prefix"], capture_output=True, text=True)
    base = "origin/main" if subprocess.run(git + ["rev-parse", "--verify", "-q", "origin/main"],
                                           capture_output=True).returncode == 0 else "HEAD"
    landed = subprocess.run(git + ["show", f"{base}:{prefix.stdout.strip()}rules.toml"], capture_output=True, text=True)
    if prefix.returncode == 0 and landed.returncode == 0:
        landed_ids = {rule.get("id") for rule in tomllib.loads(landed.stdout).get("rule", [])}
        errors += [f"rules.toml {rule.get('id')}: a new rule must cite incidents.jsonl incidents from at least 2 sessions"
                   for rule in rules if rule.get("id") not in landed_ids
                   and len({sessions[i] for i in rule.get("incidents", []) if sessions.get(i)}) < 2]
print("\n".join(errors) if errors else f"{len(registered)} global rules match AGENTS.md bullets 1:1, {len(rules)} placed")
sys.exit(1 if errors else 0)
PY
}

harness_vocabulary() {
  local repo top found=0 hits
  for repo in "$@"; do
    top="$(git -C "$repo" rev-parse --show-toplevel 2>/dev/null)" || continue
    hits="$(git -C "$top" ls-files -- 'socrates/*')"
    if [ -n "$hits" ]; then
      printf '%s: the harness lives under harness/, not socrates/: %s\n' "$top" "$(head -n 1 <<<"$hits")"
      found=1
    fi
    hits="$(git -C "$top" grep -l -E -e '(^|[^A-Za-z0-9_-])socrates/' -e 'Socrates is the agent machinery' -- . \
      ':!*.jsonl' ':!agents/incident-baseline.toml' ':!agents/replay/placements/*' \
      ':!harness/scripts/agent-config-check.sh' ':!harness/scripts/agent-config-check.test.sh' 2>/dev/null || true)"
    if [ -n "$hits" ]; then
      printf '%s: names a socrates/ path or calls Socrates the agent machinery (Socrates is the workforce, North the harness): %s\n' \
        "$top" "$(tr '\n' ' ' <<<"$hits")"
      found=1
    fi
  done
  [ "$found" = 0 ] && printf 'harness vocabulary: no socrates/ path and no "Socrates is the agent machinery"\n'
  return "$found"
}

if [[ "${BASH_SOURCE[0]}" != "$0" ]]; then
  return 0
fi

NORTH="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIRN="$(jq -er '."nixos-config" // empty' <<<"${NORTH_REPO_ROOTS:-null}" 2>/dev/null || printf '%s' "$HOME/code/nixos-config/main")"
FIRN_INTEGRATION="$FIRN/modules/north-profile/firn"
OPERATOR="${AGENTS_OPERATOR_DIR:-$HOME/code/south/main/agents}"
CODEX="$FIRN/dotfiles/codex"
LIVE_AGENT_ROOT="${NORTH_AGENT_STATE_ROOT:-$HOME/.local/state/north/agents}"
CODEX_REQUIREMENTS="$FIRN/modules/codex/requirements.toml"
LOCAL=0
VERBOSE=0
POLICY_ONLY=0
# This repository declares Tom's machine profile; CI's HOME is runner scratch.
for arg in "$@"; do
  case "$arg" in
    --local) LOCAL=1 ;;
    --policy-only) POLICY_ONLY=1 ;;
    --verbose|-v) VERBOSE=1 ;;
    *) printf 'usage: %s [--local] [--policy-only] [--verbose]\n' "$0" >&2; exit 2 ;;
  esac
done

if [ "$POLICY_ONLY" -eq 1 ]; then
  run_agent_policy_contract "$FIRN" "$LOCAL"
  exit $?
fi

fail=0
warn=0
details=()
ok_detail() { details+=("ok: $*"); }
note() { [ "$VERBOSE" -eq 0 ] || printf '  note: %s\n' "$*"; }
bad() { printf '  FAIL: %s\n' "$*" >&2; fail=$((fail + 1)); }
soft() { printf '  warn: %s\n' "$*" >&2; warn=$((warn + 1)); }
group() {
  local name="$1" summary="$2" before="$3"
  if [ "$fail" -eq "$before" ]; then printf '✓ %-13s %s\n' "$name" "$summary"
  else printf '✗ %-13s %s\n' "$name" "$summary" >&2; fi
  if [ "$VERBOSE" -eq 1 ]; then
    local line
    for line in "${details[@]}"; do printf '  %s\n' "$line"; done
  fi
  details=()
}
provider_group() {
  local name="$1" before="$2"
  shift 2
  if [ "$fail" -eq "$before" ]; then printf '✓ %s\n' "$name"
  else printf '✗ %s\n' "$name" >&2; fi
  local line
  for line in "$@"; do printf '  %s\n' "$line"; done
  if [ "$VERBOSE" -eq 1 ]; then
    for line in "${details[@]}"; do printf '    %s\n' "$line"; done
  fi
  details=()
}
need_json() {
  local file="$1" label="$2"
  if jq -e . "$file" >/dev/null 2>&1; then ok_detail "$label is valid JSON"
  else bad "$label is not valid JSON: $file"; return 1; fi
}
need_toml() {
  local file="$1" label="$2"
  if "$AGENT_CONFIG_PYTHON" -c 'import sys, tomllib; tomllib.load(open(sys.argv[1], "rb"))' "$file" >/dev/null 2>&1; then
    ok_detail "$label is valid TOML"
  else
    bad "$label is not valid TOML: $file"
    return 1
  fi
}
need_yaml() {
  # Strict parse only when a YAML parser is present (PyYAML is not stdlib and
  # is absent from the minimal system python / CI). When absent, structural
  # grep assertions below carry the load — this never hard-fails on a missing
  # parser, only on a genuinely malformed document.
  local file="$1" label="$2"
  [ -f "$file" ] || { bad "$label is missing: $file"; return 1; }
  if "$AGENT_CONFIG_PYTHON" -c 'import yaml' >/dev/null 2>&1; then
    if "$AGENT_CONFIG_PYTHON" -c 'import sys, yaml; yaml.safe_load(open(sys.argv[1]))' "$file" >/dev/null 2>&1; then
      ok_detail "$label is valid YAML"
    else
      bad "$label is not valid YAML: $file"
      return 1
    fi
  else
    note "$label YAML strict-parse skipped (no PyYAML); structural checks apply"
  fi
}
printf 'agent harness check%s\n' "$([ "$LOCAL" -eq 1 ] && printf ' (local)' || true)"

before=$fail
if policy_output="$(run_agent_policy_contract "$FIRN" "$LOCAL" 2>&1)"; then
  ok_detail "$policy_output"
else
  bad "$policy_output"
fi
group policy 'surface ownership · provider wiring generated from guard[]' "$before"

# North-composed constitution plus hook/skill implementations from each owner.
before=$fail
hook_count=0
if command -v shellcheck >/dev/null 2>&1; then
  while IFS= read -r hook; do
    hook_count=$((hook_count + 1))
    if output="$(shellcheck -S warning "$hook" 2>&1)"; then
      ok_detail "shellcheck ${hook##*/}"
    else bad "shellcheck ${hook##*/}:\n$output"; fi
  done < <(find "$NORTH/harness/hooks" -maxdepth 1 -type f -name '*.sh' ! -name '*.test.sh' -print | sort)
else bad "shellcheck is required to lint shared hooks"; fi
skill_count=0
for skill_root in \
  "$NORTH/harness/skills" \
  "$LIVE_AGENT_ROOT/current/skills/shared" \
  "$FIRN_INTEGRATION/skills"; do
  if [ ! -d "$skill_root" ]; then
    if [ "$LOCAL" -eq 1 ]; then
      bad "composed skill owner root is missing: $skill_root"
    else
      note "external skill owner root unavailable in repository-only mode: $skill_root"
    fi
    continue
  fi
  while IFS= read -r skill; do
    skill_count=$((skill_count + 1))
    if [ "$(head -n 1 "$skill")" = '---' ]; then ok_detail "${skill%/SKILL.md} has frontmatter"
    else soft "${skill#"$FIRN"/} lacks SKILL.md frontmatter"; fi
  done < <(find "$skill_root" -mindepth 2 -maxdepth 2 -name SKILL.md -type f -print | sort)
done
if [ -s "$OPERATOR/AGENTS.md" ]; then
  ok_detail "global AGENTS.md owner source present"
else
  bad "global AGENTS.md owner source is missing or empty"
fi
if instruction_result=$("$NORTH/bin/agent-instruction-check" --repo "$FIRN" --ref HEAD 2>&1); then
  ok_detail "$instruction_result"
else
  bad "$instruction_result"
fi
north_profile_module="$FIRN/modules/north-profile/default.nix"
if grep -Fq '/.local/state/north/agents/current/instructions/shared/AGENTS.md"' "$north_profile_module"; then
  ok_detail "~/.agents/AGENTS.md is wired to North-generation instructions"
else
  bad "~/.agents/AGENTS.md must be wired to the current North activation generation"
fi
if grep -Fq '/.local/state/north/agents/current/skills/shared"' "$north_profile_module"; then
  ok_detail 'shared agent skills are wired to the current North projection'
else
  bad 'shared agent skills must be wired to the current North activation generation'
fi
if grep -Fq '/.local/state/north/agents/current/provider-hooks"' "$north_profile_module"; then
  ok_detail 'shared agent hooks are wired to the current North provider hooks'
else
  bad 'shared agent hooks must be wired to the current North activation generation'
fi
if grep -Fq '/.local/state/north/agents/current/instructions/code/AGENTS.md"' "$north_profile_module"; then
  ok_detail 'code-root instructions are wired to North-generation instructions'
else
  bad 'code-root instructions must be wired to the current North activation generation'
fi
if rg -n 'agent-profile|\.config/agents|profiles/tom|\.agents/docs' \
  "$north_profile_module" >/dev/null; then
  bad 'north-profile module still declares a retired agent projection'
else
  ok_detail 'north-profile module contains no retired agent projection'
fi
if [ "$LOCAL" -eq 1 ]; then
  canonical_link "$HOME/.agents/AGENTS.md" "$LIVE_AGENT_ROOT/current/instructions/shared/AGENTS.md" "$HOME/.agents/AGENTS.md"
  canonical_link "$HOME/.agents/hooks" "$LIVE_AGENT_ROOT/current/provider-hooks" "$HOME/.agents/hooks"
  canonical_link "$HOME/.agents/skills" "$LIVE_AGENT_ROOT/current/skills/shared" "$HOME/.agents/skills"
  canonical_link "$HOME/code/AGENTS.md" "$LIVE_AGENT_ROOT/current/instructions/code/AGENTS.md" "$HOME/code/AGENTS.md"
  if [ -e "$HOME/.agents/docs" ] || [ -L "$HOME/.agents/docs" ]; then
    bad "$HOME/.agents/docs must be absent because the generation has no docs artifact"
  else
    ok_detail "$HOME/.agents/docs is absent"
  fi
fi
group shared "$hook_count owner hooks linted · $skill_count owner skills · North-generation instructions" "$before"

validate_codex_managed_policy() {
  if ! need_toml "$CODEX_REQUIREMENTS" 'Codex managed requirements'; then return; fi
  CODEX_MANAGED_BINDINGS="$(
    codex_managed_policy_binding_count "$CODEX_REQUIREMENTS" 2>/dev/null
  )" || CODEX_MANAGED_BINDINGS=''
  if [ -n "$CODEX_MANAGED_BINDINGS" ]; then
    ok_detail "Codex managed policy has $CODEX_MANAGED_BINDINGS bindings generated from guard[]"
  else
    bad 'Codex managed requirements are unreadable'
  fi

  local module="$FIRN/modules/codex/default.nix"
  local live resolved adapter expected_adapter
  local -a provider_adapters
  local adapters_out
  adapters_out="$(north agent-policy-contract --north "$NORTH" --repo "$FIRN" --codex-adapters)" &&
    mapfile -t provider_adapters <<<"$adapters_out" || bad 'Codex provider adapter list could not be computed'
  if grep -Fq 'path = "${flakeRoot}/modules/codex/requirements.toml";' "$module"; then :
  else bad 'Codex module does not install its managed requirements'; fi
  local runtime package binary
  local -a runtimes=(
    'bash|pkgs.bash|bash'
    'cat|pkgs.coreutils|cat'
    'env|pkgs.coreutils|env'
    'git|pkgs.git|git'
    'mktemp|pkgs.coreutils|mktemp'
    'python3|pkgs.python3|python3'
    'rm|pkgs.coreutils|rm'
    'timeout|pkgs.coreutils|timeout'
  )
  for source in "${runtimes[@]}"; do
    IFS='|' read -r runtime package binary <<<"$source"
    if grep -Fq "\"codex/hooks/runtime/$runtime\".source = \"\${$package}/bin/$binary\";" "$module"; then :
    else bad "Codex module does not bind exact runtime $runtime from $package"; fi
  done
  if ! grep -Fq 'codexUpstreamPkg' "$module" &&
     ! grep -Fq 'codexPkg' "$module" &&
     ! grep -Fq 'environment.systemPackages' "$module" &&
     ! grep -Fq '"codex/runtime"' "$module"; then
    ok_detail 'Codex stays out of the NixOS system closure and uses its independently promoted user runtime'
  else
    bad 'Codex module must not install Codex or expose a Codex runtime through the NixOS system closure'
  fi
  if rg -q '\bmode\s*=' "$module"; then
    bad 'Codex hook sources must remain /etc symlinks into /nix/store; explicit mode copies are forbidden'
  fi

  if [ "$LOCAL" -eq 1 ]; then
    local generation_exact=1
    if cmp -s "$CODEX_REQUIREMENTS" /etc/codex/requirements.toml; then :
    else
      generation_exact=0
      bad 'Codex managed requirements are not the current /etc generation'
    fi
    for adapter in "${provider_adapters[@]}"; do
      live="/etc/codex/hooks/$adapter"
      expected_adapter="$LIVE_AGENT_ROOT/current/provider-hooks/$adapter"
      if [ -L "$live" ] && [ "$(readlink "$live")" = "$expected_adapter" ]; then :
      else
        generation_exact=0
        bad "Codex provider adapter $live is not the stable link to $expected_adapter"
      fi
    done
    for source in "${runtimes[@]}"; do
      IFS='|' read -r runtime package binary <<<"$source"
      live="/etc/codex/hooks/runtime/$runtime"
      resolved="$(readlink -f "$live" 2>/dev/null || true)"
      if [ -x "$live" ] && [[ "$resolved" = /nix/store/* ]]; then :
      else
        generation_exact=0
        bad "Codex runtime $live is missing, non-executable, or not store-backed"
      fi
    done
    local interactive_codex user_codex
    interactive_codex="$(command -v codex 2>/dev/null || true)"
    user_codex="$HOME/.local/lib/codex/current/bin/codex"
    interactive_codex="$(readlink -f "$interactive_codex" 2>/dev/null || true)"
    if [ -x "$user_codex" ]; then
      ok_detail 'Codex user runtime is installed and executable outside the NixOS system closure'
    else
      generation_exact=0
      bad 'Codex user runtime is missing or non-executable; run codex-runtime-update'
    fi
    if [ -n "$interactive_codex" ] &&
       [ "$interactive_codex" = "$(readlink -f "$user_codex" 2>/dev/null || true)" ]; then
      ok_detail 'Interactive Codex directly resolves to the promoted user runtime'
    elif [ -n "$interactive_codex" ]; then
      ok_detail 'Interactive Codex uses a distinct user launcher over the promoted user runtime'
    else
      soft 'Interactive Codex is absent from PATH; the promoted user runtime remains independently executable'
    fi
    if [ "$generation_exact" -eq 1 ]; then
      CODEX_HOOK_PROVENANCE='North activation generation · immutable Firn runtime tools'
      ok_detail 'Codex hook deployment is exact to the current North activation generation'
    else
      CODEX_HOOK_PROVENANCE='generation drift detected'
    fi
  else
    CODEX_HOOK_PROVENANCE='immutable /nix/store generation deferred to --local'
  fi
}

before=$fail
need_toml "$CODEX/config.toml" 'Codex config'
if grep -Fq 'options.myConfig.modules.codex = {' "$FIRN/modules/codex/default.nix" &&
   grep -Fq 'configFile = lib.mkOption {' "$FIRN/modules/codex/default.nix" &&
   grep -Fq 'path = "${flakeRoot}/dotfiles/codex/config.toml";' "$FIRN/modules/codex/default.nix" &&
   grep -Fq 'cfg = config.myConfig.modules.codex;' "$FIRN/modules/codex/default.nix" &&
   grep -Fq 'codexConfigFile = cfg.configFile;' "$FIRN/modules/codex/default.nix" &&
   grep -Fq '".codex/config.toml".source = renderedCodexConfigFile;' "$FIRN/modules/codex/default.nix" &&
   grep -Fq 'context = builtins.fromJSON (builtins.readFile ../north-profile/context.json);' "$FIRN/modules/codex/default.nix" &&
   grep -Fq 'model_context_window = ${builtins.toString contextWindow}' "$FIRN/modules/codex/default.nix" &&
   grep -Fq 'model_auto_compact_token_limit = ${builtins.toString context.autoCompactTokenLimit}' "$FIRN/modules/codex/default.nix" &&
   grep -Fxq '    + builtins.readFile codexConfigFile' "$FIRN/modules/codex/default.nix" &&
   ! rg -q 'code/nixos-config/dotfiles/codex/config\.toml' \
     "$FIRN/modules/codex/default.nix"; then
  ok_detail 'Codex config option defaults to the committed flake source and supplies the generation-owned store source'
else
  bad 'Codex config option must default to the committed flake source and supply the generation-owned store source'
fi
validate_codex_managed_policy
codex_bindings="${CODEX_MANAGED_BINDINGS:-invalid}"
codex_hook_provenance="${CODEX_HOOK_PROVENANCE:-declaration drift detected}"
if grep -q '^\[mcp_servers\.north\]' "$CODEX/config.toml"; then
  bad 'Codex config still declares the retired North MCP server'
else
  ok_detail 'Codex config has no North MCP server'
fi
if [ "$LOCAL" -eq 1 ]; then
  codex_host_flake="$FIRN"
  if [ -f "${OPERATOR%/agents}/flake.nix" ]; then
    codex_host_flake="${OPERATOR%/agents}"
  fi
  selected_codex_base=$(nix eval --raw --no-write-lock-file \
    "path:$codex_host_flake#nixosConfigurations.whiterabbit.config.myConfig.modules.codex.configFile")
  if [ -L "$HOME/.codex/config.toml" ] &&
     [[ "$(readlink -f "$HOME/.codex/config.toml")" == "${NIX_STORE_PREFIX:-/nix/store}/"* ]] &&
     "$AGENT_CONFIG_PYTHON" - "$selected_codex_base" "$HOME/.codex/config.toml" "$FIRN/modules/north-profile/context.json" <<'PYCONTEXT'
import json, pathlib, sys, tomllib
base, rendered, context_path = map(pathlib.Path, sys.argv[1:])
context = json.loads(context_path.read_text())
window = min(context["window"], context["modelMaxContextWindow"])
prefix = f"model_context_window = {window}\nmodel_auto_compact_token_limit = {context['autoCompactTokenLimit']}\n"
assert rendered.read_bytes() == prefix.encode() + base.read_bytes()
expected = tomllib.loads(base.read_text())
expected.update(model_context_window=window, model_auto_compact_token_limit=context["autoCompactTokenLimit"])
assert tomllib.loads(rendered.read_text()) == expected
PYCONTEXT
  then
    ok_detail 'Codex store config preserves every base byte and setting after the shared context prefix'
  else
    bad 'Codex store config must preserve the selected base and shared context values'
  fi
  canonical_link "$HOME/.codex/AGENTS.md" "$LIVE_AGENT_ROOT/current/instructions/codex/AGENTS.md" "$HOME/.codex/AGENTS.md"
  if [ -d "$HOME/.codex/skills" ] && [ ! -L "$HOME/.codex/skills" ]; then
    ok_detail 'Codex skills remains a provider/user-owned directory'
  else
    bad 'Codex skills must remain a real directory; North owns only exact compatibility links inside it'
  fi
fi
provider_group Codex "$before" \
  "Hooks       $codex_bindings managed authoritative bindings" \
  'Identity    managed lanes harness-owned · pinned native fallback → openai' \
  'Topology    sole policy: managed /etc/codex/hooks' \
  "Hook source $codex_hook_provenance" \
  'Bootstrap   static config parsed' \
  'MCP         North absent'

before=$fail
routines_args=(--no-live)
[ "$LOCAL" -eq 0 ] || routines_args=()
if routines_out="$("$AGENT_CONFIG_PYTHON" -I "$NORTH/harness/lib/routines.py" list "${routines_args[@]}" 2>&1)"; then
  ok_detail "$(printf '%s\n' "$routines_out" | tail -n 1)"
else
  bad "$(printf '%s\n' "$routines_out" | grep -E '^(INVALID|EXPIRED|UNREGISTERED|WRONG HOST)' || printf '%s' "$routines_out")"
fi
group routines "every recurring job has an operator routines entry$([ "$LOCAL" -eq 1 ] && printf ', live timers included' || true)" "$before"

before=$fail
if budget_out="$(policy_gate "$FIRN" 2>&1)"; then
  ok_detail "$budget_out"
  budget_summary="$budget_out"
else
  bad "$budget_out"
  budget_summary='playbook body over 500 lines'
fi
group skills "$budget_summary" "$before"

before=$fail
if rules_out="$(rule_register "$OPERATOR" 2>&1)"; then
  ok_detail "$rules_out"
  rules_summary="$rules_out"
else
  bad "$rules_out"
  rules_summary='AGENTS.md and rules.toml diverge'
fi
group rules "$rules_summary" "$before"

before=$fail
if vocabulary_out="$(harness_vocabulary "$NORTH" "$OPERATOR/.." 2>&1)"; then
  ok_detail "$vocabulary_out"
else
  bad "$vocabulary_out"
fi
group vocabulary 'harness/ paths · Socrates names the workforce' "$before"

# --- worktree layout -------------------------------------------------------
# A rule with no detector silently stops being true. On 2026-07-29 a sweep found
# 63 worktrees across FOUR conventions at once, plus seven clones of north at
# ~/code root and orphaned trees whose gitdir no longer existed — after the
# layout had been written down and restated repeatedly. This is the detector.
if [ "$LOCAL" -eq 1 ] && command -v north >/dev/null 2>&1; then
  if worktree_layout_out="$(north worktree-layout-check 2>&1)"; then
    printf '\u2713 %s\n' "$worktree_layout_out"
  else
    bad "worktree layout drift — run north worktree-layout-check for the list"
    printf '%s\n' "$worktree_layout_out" >&2
  fi
fi

if [ "$fail" -ne 0 ]; then printf 'agent-config-check: FAILED\n' >&2; exit 1; fi
if [ "$warn" -gt 0 ]; then printf 'agent-config-check: passed with %s warning(s)\n' "$warn"
else printf 'agent-config-check: all green\n'; fi
