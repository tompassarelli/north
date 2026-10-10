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

solo="$scratch/solo.json"
cat >"$solo" <<'JSON'
{"version":1,"priority":[],"nodes":[
 {"id":"muove-lead","role":"lead","domain":"galileo","parent":"proxy","provider":"claude","session":"claude-muove","depth":1,"budget":2,"state":"active"},
 {"id":"telemetry-lead","role":"lead","domain":"tooling","parent":"proxy","provider":"claude","session":"claude-telemetry","depth":1,"budget":2,"state":"active"},
 {"id":"telemetry-codex-lead","role":"sub-lead","domain":"tooling","parent":"telemetry-lead","provider":"codex","session":"thr-telemetry","depth":2,"budget":1,"state":"active"}
]}
JSON

none="$scratch/none.json"
printf '{"version":1,"priority":[],"nodes":[{"id":"muove-lead","role":"lead","domain":"galileo","parent":"proxy","provider":"claude","session":"claude-muove","depth":1,"budget":2,"state":"active"}]}\n' >"$none"

# Definitions only: every line above the dispatch `cmd=` line, so the real functions run.
defs="$scratch/defs.sh"
sed '/^cmd=/,$d' "$tool" >"$defs"

# resolve ORG CALLER: prints the resolved thread (or the refusal) for one caller node.
resolve() {
  (
    unset CODEX_LEAD_THREAD
    export AGENTS_ORG_FILE="$1"
    if [ -n "$2" ]; then export AGENT_ORG_NAME="$2"; else unset AGENT_ORG_NAME; fi
    # shellcheck source=/dev/null
    source "$defs"
    thread
  )
}

expect_thread() { # name, org, caller, expected
  local out err
  err="$scratch/err"
  out=$(resolve "$2" "$3" 2>"$err") || fail "$1: resolution failed: $(cat "$err")"
  [ "$out" = "$4" ] || fail "$1: resolved '$out', expected '$4'"
}

expect_refusal() { # name, org, caller, count
  local out err
  err="$scratch/err"
  if out=$(resolve "$2" "$3" 2>"$err"); then fail "$1: resolved '$out' instead of refusing"; fi
  grep -qx "codex-lead: $4 Codex leads are live; set CODEX_LEAD_THREAD to one of:" "$err" ||
    fail "$1: missing refusal header for $4 leads: $(cat "$err")"
}

expect_refusal ambiguous-no-caller-child "$org" muove-lead 3
err="$scratch/err"; resolve "$org" muove-lead 2>"$err" || true
grep -qx 'muove-codex-lead  thr-muove' "$err" || fail "list lacks muove-codex-lead"
grep -qx 'telemetry-codex-lead  thr-telemetry' "$err" || fail "list lacks telemetry-codex-lead"
grep -qx 'smashcraft-codex-lead  thr-smashcraft' "$err" || fail "list lacks smashcraft-codex-lead"
grep -q 'old-codex-lead' "$err" && fail "list includes a finished lead"
expect_refusal ambiguous-no-caller "$org" "" 3

expect_thread caller-has-codex-child "$org" telemetry-lead thr-telemetry
expect_thread caller-is-codex-lead "$org" muove-codex-lead thr-muove
expect_thread single-live-lead "$solo" muove-lead thr-telemetry

(
  unset AGENT_ORG_NAME
  export AGENTS_ORG_FILE="$org" CODEX_LEAD_THREAD=thr-env
  source "$defs"
  [ "$(thread)" = thr-env ] || fail "CODEX_LEAD_THREAD did not win"
)

if out=$(resolve "$none" muove-lead 2>"$scratch/err"); then fail "zero live leads resolved '$out'"; fi
grep -q 'no live Codex lead' "$scratch/err" || fail "zero live leads: wrong message: $(cat "$scratch/err")"

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
grep -q 'Codex leads are live' "$scratch/send.err" || fail "send did not refuse: $(cat "$scratch/send.err")"
[ ! -e "$scratch/ss.log" ] || fail "send touched the daemon while refusing"

if env -u CODEX_LEAD_THREAD AGENTS_ORG_FILE="$org" AGENT_ORG_NAME=telemetry-lead PATH="$stubs:$PATH" \
    "$tool" send hi >/dev/null 2>"$scratch/send.err"; then
  fail "send reached a daemon stub that reports none"
fi
grep -q 'no Codex app-server daemon' "$scratch/send.err" ||
  fail "send from telemetry-lead did not resolve its codex sub-lead: $(cat "$scratch/send.err")"
[ "$(wc -l <"$scratch/ss.log")" -eq 1 ] || fail "expected exactly one daemon lookup for the resolved lead"

echo "codex-lead.test.sh: all assertions passed"
