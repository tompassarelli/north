#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tool="$repo/bin/codex-lead"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/codex-lead-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT

fail() { printf 'codex-lead.test.sh:%s: %s\n' "${BASH_LINENO[0]}" "$1" >&2; exit 1; }

org="$scratch/org.json"
cat >"$org" <<'JSON'
{"version":1,"priority":["galileo"],"nodes":[
 {"id":"muove-lead","role":"lead","domain":"galileo","parent":"proxy","provider":"claude","session":"claude-muove","depth":1,"budget":2,"state":"active"},
 {"id":"muove-codex-lead","role":"lead","domain":"galileo","parent":"proxy","provider":"codex","session":"thr-muove","depth":1,"budget":2,"state":"active"},
 {"id":"telemetry-lead","role":"lead","domain":"tooling","parent":"proxy","provider":"claude","session":"claude-telemetry","depth":1,"budget":2,"state":"active"},
 {"id":"telemetry-codex-lead","role":"sub-lead","domain":"tooling","parent":"telemetry-lead","provider":"codex","session":"thr-telemetry","depth":2,"budget":1,"state":"active"},
 {"id":"smashcraft-lead","role":"lead","domain":"smashcraft","parent":"proxy","provider":"claude","session":"claude-smashcraft","depth":1,"budget":2,"state":"active"},
 {"id":"smashcraft-codex-lead","role":"sub-lead","domain":"smashcraft","parent":"smashcraft-lead","provider":"codex","session":"thr-smashcraft","depth":2,"budget":1,"state":"active"},
 {"id":"old-codex-lead","role":"lead","domain":"galileo","parent":"proxy","provider":"codex","session":"thr-old","depth":1,"budget":2,"state":"finished"}
]}
JSON

# Binary path: a stub `ss` reports no daemon and logs each call, so a
# resolution that wrongly succeeds would be visible without any live send.
stubs="$scratch/stubs"
mkdir -p "$stubs"
printf '#!/bin/sh\necho called >>"%s"\n' "$scratch/ss.log" >"$stubs/ss"
chmod +x "$stubs/ss"

if env -u CODEX_LEAD_THREAD AGENTS_ORG_FILE="$org" AGENT_ORG_NAME=muove-lead PATH="$stubs:$PATH" \
    "$tool" send hi >/dev/null 2>"$scratch/send.err"; then
  fail "send with three live leads and no caller child exited 0"
fi
[ ! -e "$scratch/ss.log" ] || fail "send touched the daemon while refusing"
printf 'PASS ambiguous identity refuses before daemon lookup\n'

if env -u CODEX_LEAD_THREAD AGENTS_ORG_FILE="$org" AGENT_ORG_NAME=telemetry-lead PATH="$stubs:$PATH" \
    "$tool" send hi >/dev/null 2>"$scratch/send.err"; then
  fail "send reached a daemon stub that reports none"
fi
[ -s "$scratch/ss.log" ] || fail "resolved lead did not look up its daemon"
printf 'PASS caller child identity reaches daemon lookup\n'

echo "codex-lead.test.sh: all assertions passed"
