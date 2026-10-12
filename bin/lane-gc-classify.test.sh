#!/usr/bin/env bash
set -euo pipefail

target="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lane-gc"
scratch=$(mktemp -d)
holder=""
trap '[ -z "$holder" ] || kill "$holder" 2>/dev/null; rm -rf "$scratch"' EXIT
export HOME=$scratch/home GIT_CONFIG_GLOBAL=$scratch/gitconfig GIT_CONFIG_NOSYSTEM=1
export AGENTS_ORG_FILE=$scratch/org.json LANE_GC_HANDOFF_DIR=$scratch/handoffs LANE_GC_STALE_FILE=$scratch/stale.json
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
cat >"$AGENTS_ORG_FILE" <<'JSON'
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
tips=$(for b in equiv sup wip; do git -C "$main" rev-parse "$b"; done)

fail=0
check() { if eval "$2"; then printf 'ok: %s\n' "$1"; else printf 'FAIL: %s\n' "$1"; fail=1; fi; }
run() { "$target" --root "$HOME/code" --state "$scratch/unlanded.txt" "$@" 2>&1; }
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
  lanes_intact && [ -z "$(git -C "$main" tag -l "archive/*")" ] && [ ! -e "$scratch/alerts" ]'

printf '#!/usr/bin/env bash\nwhile read -r old new ref; do case "$ref" in refs/tags/*) exit 1 ;; esac; done\n' >"$scratch/origin/north.git/hooks/pre-receive"
chmod +x "$scratch/origin/north.git/hooks/pre-receive"
out=$(run)
check 'a refused archive push leaves every lane, branch and origin branch untouched' '
  lanes_intact && git -C "$main" ls-remote --exit-code origin refs/heads/sup >/dev/null &&
  [ -z "$(git -C "$main" ls-remote origin "refs/tags/*")" ] && grep -q "kept (archive tag push unverified)" <<<"$out"'
rm "$scratch/origin/north.git/hooks/pre-receive"

out=$(run)
remote_tag() { git -C "$main" ls-remote origin "refs/tags/archive/north/$1/*" | awk '{print $1}'; }
snap=$(remote_tag wip)
tagged_gone() {
  [ ! -e "$HOME/code/north/worktrees/$1" ] && ! git -C "$main" rev-parse -q --verify "refs/heads/$1" >/dev/null &&
    [ "$(remote_tag "$1")" = "$2" ] && [ "$(git -C "$main" rev-parse -q --verify "refs/tags/archive/north/$1/${2:0:12}")" = "$2" ]
}
check 'landed-elsewhere and superseded lanes retire only behind a pushed tag at their tip, origin branch included' '
  tagged_gone equiv "$(sed -n 1p <<<"$tips")" && tagged_gone sup "$(sed -n 2p <<<"$tips")" &&
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
check 'a lead with 26 owned lanes gets one supply-alert and an inbox line, as does the proxy' '
  [ "$(wc -l <"$scratch/alerts")" -eq 1 ] && grep -q "^normal|scratch-lead: 26 owned unlanded lanes" "$scratch/alerts" &&
  grep -q "scratch-lead: 26 owned" "$LANE_GC_HANDOFF_DIR/scratch-lead-inbox.md" && grep -q "scratch-lead: 26 owned" "$LANE_GC_HANDOFF_DIR/proxy-1-inbox.md" &&
  [ ! -e "$LANE_GC_HANDOFF_DIR/north-lead-inbox.md" ]'
exit "$fail"
