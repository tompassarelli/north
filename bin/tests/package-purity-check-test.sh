#!/usr/bin/env bash
# Regression for the package purity guard (flake.nix installPhase scan).
#
# The guard rejects any embedded checkout/home/cache path in the packaged
# output, with narrow audited exceptions: the NixOS runtime entry-hint
# pointers in sdk/src/trusted-runtime.ts and the fixed bb fallback expressions
# in the packaged north/concern launchers.
# Those are root-managed symlinks that trustedStoreExecutable() still forces to
# canonicalize into the immutable /nix/store, so they never widen trust; they
# exist because managed spawns do not always inherit NORTH_GIT_BIN / NORTH_BB.
#
# This test proves the exemption is NARROW: only the exact expressions in
# their owning files are spared. It extracts the impurity_pattern and sanctioned
# allowlist regexes straight from flake.nix so it tracks the real guard rather
# than a hand-copied duplicate that could silently drift.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
flake="$repo_root/flake.nix"

impurity_pattern=$(sed -n "s/^[[:space:]]*impurity_pattern='\(.*\)'\$/\1/p" "$flake")
sanctioned=$(sed -n "s/^[[:space:]]*sanctioned='\(.*\)'\$/\1/p" "$flake")
[ -n "$impurity_pattern" ] || { echo "FAIL: impurity_pattern not found in flake.nix" >&2; exit 1; }
[ -n "$sanctioned" ] || { echo "FAIL: sanctioned allowlist not found in flake.nix" >&2; exit 1; }

# Mirror the guard's residual computation exactly.
scan() {
  LC_ALL=C rg --hidden -n "$impurity_pattern" "$1" | LC_ALL=C rg -v "$sanctioned" || true
}

scan_tracked_sdk() {
  (cd "$repo_root"
   git ls-files -z sdk/src \
     | xargs -0 -r rg --hidden -n "$impurity_pattern" \
     | LC_ALL=C rg -v "$sanctioned" || true)
}

work=$(mktemp -d)
trap 'rm -rf "${work:?}"' EXIT

pass() { echo "ok: $1"; }
expect_clean() { # dir label
  [ -z "$(scan "$1")" ] || { echo "FAIL: $2" >&2; scan "$1" >&2; exit 1; }
  pass "$2"
}
expect_flagged() { # dir label
  [ -n "$(scan "$1")" ] || { echo "FAIL: $2" >&2; exit 1; }
  pass "$2"
}

# A: the exact sanctioned pointer lines are exempted (guard passes clean).
mkdir -p "$work/a/sdk/src"
cat > "$work/a/sdk/src/trusted-runtime.ts" <<'EOF'
    process.env.NORTH_GIT_BIN,
    "/run/current-system/sw/bin/git",
    "/run/current-system/sw/bin/bb",
EOF
expect_clean "$work/a" "sanctioned git/bb entry-hint pointers are exempted"

# B: a real home/checkout path in the SAME file stays fatal.
mkdir -p "$work/b/sdk/src"
cat > "$work/b/sdk/src/trusted-runtime.ts" <<'EOF'
    "/run/current-system/sw/bin/git",
    "/home/tom/code/north/leak",
EOF
expect_flagged "$work/b" "a home path inside trusted-runtime.ts is still fatal"

# C: a non-git/bb system-profile target in that file stays fatal.
mkdir -p "$work/c/sdk/src"
printf '    "/run/current-system/sw/bin/evil",\n' > "$work/c/sdk/src/trusted-runtime.ts"
expect_flagged "$work/c" "a non-git/bb system-profile path is not exempted"

# D: the sanctioned literal in ANY OTHER file stays fatal.
mkdir -p "$work/d/sdk/src"
printf '    "/run/current-system/sw/bin/git",\n' > "$work/d/sdk/src/other.ts"
expect_flagged "$work/d" "the exemption does not apply outside trusted-runtime.ts"

# E: package-eligible SDK source carries no UNSANCTIONED impurity. Untracked
# scratch files are intentionally outside the Git-backed Nix source.
[ -z "$(scan_tracked_sdk)" ] || {
  echo "FAIL: tracked sdk/src has unsanctioned impurity" >&2
  scan_tracked_sdk >&2
  exit 1
}
pass "tracked sdk/src has no unsanctioned impurity"

# F: north-data is a runtime corpus directory, not the north source checkout.
mkdir -p "$work/f/cli"
printf '    (str home "/code/north-data/coordination.framlog")\n' > "$work/f/cli/coord.clj"
expect_clean "$work/f" "runtime north-data path is not mistaken for the north checkout"

# G: the actual checkout root and its descendants remain fatal.
mkdir -p "$work/g/cli"
printf '    (str home "/code/north/cli/coord.clj")\n' > "$work/g/cli/coord.clj"
expect_flagged "$work/g" "north checkout descendants remain fatal"

# H: only the packaged north/concern wrappers' exact fixed-bb fallback
# expressions are exempted. Source launchers retain these entry hints for a
# promoted checkout; package wrappers set NORTH_BB before they can be reached.
mkdir -p "$work/h/bin"
cat > "$work/h/bin/.north-wrapped" <<'EOF'
elif [ -x /run/current-system/sw/bin/bb ]; then
  BB="/run/current-system/sw/bin/bb"
  echo "north: cannot find babashka — tried \$NORTH_BB, PATH, /run/current-system/sw/bin/bb" >&2
EOF
cat > "$work/h/bin/.concern-wrapped" <<'EOF'
elif [ -x /run/current-system/sw/bin/bb ]; then
  BB="/run/current-system/sw/bin/bb"
  echo "concern: cannot find babashka — tried \$NORTH_BB, PATH, /run/current-system/sw/bin/bb" >&2
EOF
expect_clean "$work/h" "packaged launcher bb fallback expressions are exempted"

# I: neither wrapper receives a blanket exemption for arbitrary uses.
mkdir -p "$work/i/bin"
printf 'exec "/run/current-system/sw/bin/bb"\n' > "$work/i/bin/.north-wrapped"
expect_flagged "$work/i" "north wrapper may not use the bb hint outside its fallback"

# J: the same fallback expression in any other packaged launcher stays fatal.
mkdir -p "$work/j/bin"
printf '  BB="/run/current-system/sw/bin/bb"\n' > "$work/j/bin/.other-wrapped"
expect_flagged "$work/j" "bb fallback exemption does not apply to another launcher"

echo "PASS: purity-guard allowlist is expression- and file-scoped"
