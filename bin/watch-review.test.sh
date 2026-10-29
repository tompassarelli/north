#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/watch-review-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT

commit() {
  git -C "$1" -c user.name=t -c user.email=t@example.invalid commit -q --allow-empty -m "$2"
}

for name in moved still pathed; do
  git init -q -b main "$scratch/$name"
  mkdir -p "$scratch/$name/sub" "$scratch/$name/other"
  : >"$scratch/$name/sub/a"
  : >"$scratch/$name/other/b"
  git -C "$scratch/$name" add -A
  commit "$scratch/$name" base
done
base() { git -C "$scratch/$1" rev-parse --short HEAD; }
moved_base=$(base moved) still_base=$(base still) pathed_base=$(base pathed)
echo change >"$scratch/moved/sub/a"
git -C "$scratch/moved" add -A
commit "$scratch/moved" change
echo change >"$scratch/pathed/other/b"
git -C "$scratch/pathed" add -A
commit "$scratch/pathed" "outside the followed path"

cat >"$scratch/watch.toml" <<TOML
[[source]]
id = "moved"
repo = "$scratch/moved"
path = ""
last_reviewed = 2026-07-10
last_commit = "$moved_base"

[[source]]
id = "still"
repo = "$scratch/still"
path = ""
last_reviewed = 2026-07-10
last_commit = "$still_base"

[[source]]
id = "pathed"
repo = "$scratch/pathed"
path = "sub"
last_reviewed = 2026-07-10
last_commit = "$pathed_base"
TOML

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export WATCH_TOML="$scratch/watch.toml" WATCH_CACHE="$scratch/cache" HOME="$scratch/home"
out=$("$repo/bin/watch-review" --dry-run)
[[ "$out" == "watch-review: launch moved (1 commits, $moved_base..$(base moved))" ]] || { echo "unexpected dry run: $out" >&2; exit 1; }

cat >"$scratch/north" <<STUB
#!/usr/bin/env bash
case "\$2" in
  accounts) printf 'claude\tb\n' ;;
  list) [[ -f "$scratch/live" ]] && printf 'watch-review-moved-202609290900\tclaude\tb\trunning\n' || true ;;
  *) printf '%s\n' "\$*" >>"$scratch/launches" ;;
esac
STUB
chmod +x "$scratch/north"
WATCH_NORTH_BIN="$scratch/north" "$repo/bin/watch-review" >/dev/null
[[ $(grep -c 'fleet launch' "$scratch/launches") -eq 1 ]] || { echo "expected one launch" >&2; exit 1; }
grep -Eq -- '--name watch-review-moved-[0-9]{12} --account b' "$scratch/launches"
touch "$scratch/live"
out=$(WATCH_NORTH_BIN="$scratch/north" "$repo/bin/watch-review")
[[ "$out" == 'watch-review: moved already has a live session' ]] || { echo "live session relaunched: $out" >&2; exit 1; }
[[ $(grep -c 'fleet launch' "$scratch/launches") -eq 1 ]] || { echo 'relaunched over a live session' >&2; exit 1; }
rm "$scratch/live"

sed -i "s/last_commit = \"$moved_base\"/last_commit = \"deadbeef\"/" "$scratch/watch.toml"
if "$repo/bin/watch-review" --dry-run 2>"$scratch/err" >/dev/null; then echo "unknown last_commit accepted" >&2; exit 1; fi
grep -q 'moved: deadbeef not found' "$scratch/err"
echo watch-review ok
