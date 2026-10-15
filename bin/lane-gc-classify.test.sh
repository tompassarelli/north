#!/usr/bin/env bash
set -euo pipefail

target="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lane-gc"
scratch=$(mktemp -d)
real_git=$(command -v git)
snapshot_date=$(date -u +%FT%TZ)
holder=""
trap '[ -z "$holder" ] || kill "$holder" 2>/dev/null; rm -rf "$scratch"' EXIT
export HOME=$scratch/home GIT_CONFIG_GLOBAL=$scratch/gitconfig GIT_CONFIG_NOSYSTEM=1
export FLEET_TEST_FILE=$scratch/org.json LANE_GC_HANDOFF_DIR=$scratch/handoffs LANE_GC_STALE_FILE=$scratch/stale.json
export LANE_GC_NOW=$(date -u -d '+30 hours' +%FT%TZ) PATH=$scratch/bin:$PATH
mkdir -p "$HOME/code" "$scratch/bin" "$LANE_GC_HANDOFF_DIR"
git config --global user.name fixture
git config --global user.email fixture@example.invalid
git config --global init.defaultBranch main
git config --global advice.detachedHead false
cat >"$scratch/bin/gh" <<'SH'
#!/usr/bin/env bash
case "$*" in
  "issue view 5 -R tompassarelli/north --json state -q .state") echo CLOSED ;;
  "issue view 6 -R tompassarelli/north --json state -q .state") echo OPEN ;;
  *) exit 1 ;;
esac
SH
printf '#!/usr/bin/env bash\ncat >/dev/null\necho %s\n' "'{\"ok\":true,\"branches\":{}}'" >"$scratch/bin/ssh"
printf '#!/usr/bin/env bash\nprintf "item\\tholder\\n"\n' >"$scratch/bin/threads"
printf '#!/usr/bin/env bash\nprintf "%%s|" "$@" >>%s\necho >>%s\n' "$scratch/alerts" "$scratch/alerts" >"$scratch/bin/supply-alert"
chmod +x "$scratch/bin/"*
cat >"$scratch/bin/north" <<'NORTH'
#!/usr/bin/env bash
if [ "$1 $2" = "fleet list" ]; then
  python3 - "$FLEET_TEST_FILE" "$HOME/code" <<'PY'
import json, sys
from pathlib import Path
data = json.load(open(sys.argv[1]))
print(json.dumps([{"name": n["id"], "conversation": n.get("session") or n["id"], "parent": None,
                   "provider":"claude", "status":"working", "cwd":str(Path(sys.argv[2]) / n["domain"] / "main")}
                  for n in data.get("nodes", []) if n["id"] != "proxy-1"]))
PY
else
  exit 1
fi
NORTH
chmod +x "$scratch/bin/north"
cat >"$FLEET_TEST_FILE" <<'JSON'
{"nodes":[{"id":"north-lead","role":"lead","domain":"north"},{"id":"scratch-lead","role":"lead","domain":"galileo"},{"id":"proxy-1","role":"proxy"}]}
JSON

project() {
  git init -q --bare "$scratch/origin/$1.git"
  git clone -q "$scratch/origin/$1.git" "$HOME/code/$1/main" 2>/dev/null
  mkdir -p "$HOME/code/$1/worktrees"
  printf 'readme\n' >"$HOME/code/$1/main/README"
  printf '*.log\n' >"$HOME/code/$1/main/.gitignore"
  git -C "$HOME/code/$1/main" add README .gitignore
  git -C "$HOME/code/$1/main" commit -qm init
  git -C "$HOME/code/$1/main" push -q origin main
}
lane() {
  git -C "$main" worktree add -q "$HOME/code/north/worktrees/$1" -b "$1"
  [ -z "${2:-}" ] || { printf '%s\n' "$1" >"$HOME/code/north/worktrees/$1/$1.txt"; git -C "$HOME/code/north/worktrees/$1" add "$1.txt"; git -C "$HOME/code/north/worktrees/$1" commit -qm "$2"; }
}

project north
main=$HOME/code/north/main
lane equiv "equiv change"
printf 'other\n' >"$main/other.txt"; git -C "$main" add other.txt; git -C "$main" commit -qm other
git -C "$main" cherry-pick equiv >/dev/null; git -C "$main" push -q origin main
lane sup $'superseded change\n\nRefs north#5'
git -C "$main" push -q origin sup
lane own $'half done\n\nRefs north#5\nRefs north#6'
lane wip
printf 'edited\n' >>"$HOME/code/north/worktrees/wip/README"
printf 'new\n' >"$HOME/code/north/worktrees/wip/new.txt"
printf 'noise\n' >"$HOME/code/north/worktrees/wip/build.log"
lane held
printf 'edited\n' >>"$HOME/code/north/worktrees/held/README"
(cd "$HOME/code/north/worktrees/held" && exec sleep 300) &
holder=$!
git -C "$main" checkout -q -b loose
printf 'loose\n' >"$main/loose.txt"; git -C "$main" add loose.txt; git -C "$main" commit -qm "no issue named"
git -C "$main" checkout -q main
project galileo
git -C "$HOME/code/galileo/main" checkout -q -b unique
printf 'u\n' >"$HOME/code/galileo/main/u.txt"; git -C "$HOME/code/galileo/main" add u.txt; git -C "$HOME/code/galileo/main" commit -qm unique
git -C "$HOME/code/galileo/main" checkout -q main
for i in $(seq 1 26); do git -C "$HOME/code/galileo/main" branch "lane-$i" unique; done
git -C "$HOME/code/galileo/main" branch -q -D unique
legacy=$(git -C "$main" rev-parse equiv)
git -C "$main" tag "archive/north/equiv/${legacy:0:12}" "$legacy"
git -C "$main" push -q origin "refs/tags/archive/north/equiv/${legacy:0:12}"
cat >"$scratch/bin/git" <<SH
#!/usr/bin/env bash
args=("\$@")
last=\${args[-1]}
if [[ " \$* " == *" ls-remote origin refs/tags/archive/"* ]] && [ -f "$scratch/remote-fault" ]; then
  case \$(cat "$scratch/remote-fault") in
    missing) exit 0 ;;
    unavailable) exit 1 ;;
    mismatch)
      printf '%s\\t%s\\n' "0000000000000000000000000000000000000000" "\$last"
      exit 0 ;;
  esac
fi
exec "$real_git" "\$@"
SH
chmod +x "$scratch/bin/git"
tips=$(for b in equiv sup wip; do git -C "$main" rev-parse "$b"; done)

fail=0
checks=0
check() { checks=$((checks + 1)); if eval "$2"; then printf 'ok: %s\n' "$1"; else printf 'FAIL: %s\n' "$1"; fail=1; fi; }
run() { GIT_AUTHOR_DATE="$snapshot_date" GIT_COMMITTER_DATE="$snapshot_date" "$target" --root "$HOME/code" --state "$scratch/unlanded.txt" "$@" 2>&1; }
lanes_intact() { for b in equiv sup wip own held; do [ -d "$HOME/code/north/worktrees/$b" ] && git -C "$main" rev-parse -q --verify "refs/heads/$b" >/dev/null || return 1; done; }

out=$(run --dry-run)
check 'dry run: each lane gets its class and action, and nothing changes' '
  grep -q "north: landed-elsewhere ~/code/north/worktrees/equiv: archive to archive/north/equiv/" <<<"$out" &&
  grep -q "north: superseded ~/code/north/worktrees/sup: archive to archive/north/sup/" <<<"$out" &&
  grep -q "north: stale-dirty ~/code/north/worktrees/wip: archive to archive/north/wip/<snapshot>, then retire" <<<"$out" &&
  grep -q "north: owned ~/code/north/worktrees/own: surface to north-lead (north#5 north#6)" <<<"$out" &&
  grep -q "north: owned ~/code/north/worktrees/held: surface to north-lead" <<<"$out" &&
  grep -q "north: owned branch loose: surface to north-lead$" <<<"$out" &&
  grep -q "would alert scratch-lead: 26 owned unlanded lanes" <<<"$out" &&
  lanes_intact && [ "$(git -C "$main" tag -l "archive/*")" = "archive/north/equiv/${legacy:0:12}" ] && [ ! -e "$scratch/alerts" ]'

printf '#!/usr/bin/env bash\nwhile read -r old new ref; do case "$ref" in refs/tags/*) exit 1 ;; esac; done\n' >"$scratch/origin/north.git/hooks/pre-receive"
chmod +x "$scratch/origin/north.git/hooks/pre-receive"
printf missing >"$scratch/remote-fault"
conflict="archive/north/sup/$(git -C "$main" rev-parse --short=12 sup)"
git -C "$main" tag "$conflict" main
out=$(run)
check 'an archive conflict keeps the exact tag, lane and branch' '
  lanes_intact && [ "$(git -C "$main" rev-parse "$conflict")" = "$(git -C "$main" rev-parse main)" ] &&
  grep -q "kept (archive tag conflict)" <<<"$out"'
git -C "$main" tag -d "$conflict" >/dev/null
rm "$scratch/remote-fault"

printf missing >"$scratch/remote-fault"
out=$(run)
tag="archive/north/sup/$(git -C "$main" rev-parse --short=12 sup)"
if "$real_git" -C "$main" push -q origin "refs/tags/$tag:refs/tags/$tag" 2>"$scratch/refused.err"; then exit 1; fi
push_error=$(grep -v '^error: failed to push some refs' "$scratch/refused.err" | tail -n1)
printf 'Captured push stderr: %s\n' "$push_error"
check 'a refused archive push names the last stderr line in reason and notice and preserves lanes' '
  [[ "$push_error" == *"pre-receive hook declined"* ]] &&
  lanes_intact && git -C "$main" ls-remote --exit-code origin refs/heads/sup >/dev/null &&
  [ "$(git -C "$main" ls-remote origin "refs/tags/*" | cut -f1)" = "$legacy" ] &&
  grep -Fq "kept (archive tag push unverified: $push_error)" <<<"$out" &&
  grep -Fq "archive tag push unverified: $push_error" "$(dirname "$LANE_GC_STALE_FILE")/notices.md" &&
  python3 -c "import json,sys; sys.exit(not any(sys.argv[2] in line for line in json.load(open(sys.argv[1]))[\"notices\"]))" "$LANE_GC_STALE_FILE" "$push_error"'
git -C "$main" tag -f -a "$tag" -m 'recorded superseded archive' sup >/dev/null
annotated=$(git -C "$main" rev-parse "$tag")
rm "$scratch/origin/north.git/hooks/pre-receive"

for fault in missing mismatch unavailable; do
  printf '%s' "$fault" >"$scratch/remote-fault"
  out=$(run)
  check "$fault remote archive identity leaves every lane and branch intact" '
    lanes_intact && git -C "$main" ls-remote --exit-code origin refs/heads/sup >/dev/null &&
    grep -q "kept (archive tag push unverified)" <<<"$out"'
done
rm "$scratch/remote-fault"
out=$(run)
remote_tag() { git -C "$main" ls-remote origin "refs/tags/archive/north/$1/*" | awk 'NF == 2 { oid = $1; if ($2 ~ /\^\{\}$/) { print $1; peeled = 1; exit } } END { if (!peeled) print oid }'; }
snap=$(remote_tag wip)
tagged_gone() {
  [ ! -e "$HOME/code/north/worktrees/$1" ] && ! git -C "$main" rev-parse -q --verify "refs/heads/$1" >/dev/null &&
    [ "$(remote_tag "$1")" = "$2" ] &&
    [ "$(git -C "$main" rev-parse -q --verify "refs/tags/archive/north/$1/${2:0:12}^{commit}")" = "$2" ]
}
check 'landed-elsewhere and superseded lanes retire behind lightweight or annotated tags at their tip, origin branch included' '
  tagged_gone equiv "$(sed -n 1p <<<"$tips")" && tagged_gone sup "$(sed -n 2p <<<"$tips")" &&
  [ "$(git -C "$main" rev-parse "$tag")" = "$annotated" ] &&
  [ "$(git -C "$main" ls-remote origin "refs/tags/$tag" | cut -f1)" = "$annotated" ] &&
  [ -z "$(git -C "$main" ls-remote origin refs/heads/sup)" ]'
check 'stale-dirty work is committed on the tip with tracked and untracked changes but not ignored files' '
  [ -n "$snap" ] && [ ! -e "$HOME/code/north/worktrees/wip" ] &&
  [ "$(git -C "$main" rev-parse "$snap^")" = "$(sed -n 3p <<<"$tips")" ] &&
  [ "$(git -C "$main" show "$snap:new.txt")" = new ] && git -C "$main" show "$snap:README" | grep -q edited &&
  ! git -C "$main" cat-file -e "$snap:build.log" 2>/dev/null'
check 'owned lanes, a live-held dirty lane and an issue-less branch stay, and only they remain unlanded' '
  [ -d "$HOME/code/north/worktrees/own" ] && [ -d "$HOME/code/north/worktrees/held" ] &&
  git -C "$main" rev-parse -q --verify refs/heads/loose >/dev/null &&
  [ "$(grep -v "^#" "$scratch/unlanded.txt" | awk -F"\t" "\$2==\"north\" {print \$4}" | sort | tr "\n" " ")" = "held loose own " ] &&
  python3 -c "import json,sys; o=json.load(open(sys.argv[1]))[\"repos\"][\"north\"][\"owned\"]; sys.exit(sorted((r[\"branch\"], tuple(r[\"refs\"]), r[\"owner\"]) for r in o) != [(\"held\", (), \"north-lead\"), (\"loose\", (), \"north-lead\"), (\"own\", (\"north#5\", \"north#6\"), \"north-lead\")])" "$LANE_GC_STALE_FILE"'
check 'a Fleet parent with 26 owned lanes gets one supply-alert and no inbox writes' '
  [ "$(wc -l <"$scratch/alerts")" -eq 1 ] && grep -q "^normal|scratch-lead: 26 owned unlanded lanes" "$scratch/alerts" &&
  [ ! -e "$LANE_GC_HANDOFF_DIR/scratch-lead-inbox.md" ] && [ ! -e "$LANE_GC_HANDOFF_DIR/proxy-1-inbox.md" ] &&
  [ ! -e "$LANE_GC_HANDOFF_DIR/north-lead-inbox.md" ]'
printf 'lane-gc classification journey: %s (%d checks)\n' "$([ "$fail" = 0 ] && echo PASS || echo FAIL)" "$checks"
exit "$fail"
