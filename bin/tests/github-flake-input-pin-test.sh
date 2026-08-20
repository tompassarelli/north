#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PIN="$ROOT/bin/github-flake-input-pin"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

write_lock() {
  local input="$1" type="$2" owner="$3" repo="$4" rev="$5" hash="$6"
  jq -cn \
    --arg input "$input" \
    --arg type "$type" \
    --arg owner "$owner" \
    --arg repo "$repo" \
    --arg rev "$rev" \
    --arg hash "$hash" \
    '{nodes:{root:{inputs:{($input):"source_2"}},source_2:{locked:{type:$type,owner:$owner,repo:$repo,rev:$rev,narHash:$hash}}}}' \
    >"$TMP/flake.lock"
}

rev=0123456789abcdef0123456789abcdef01234567
write_lock store-source github example store "$rev" 'sha256-YWJjZA=='
[[ "$("$PIN" "$TMP/flake.lock" store-source repository)" == example/store ]]
[[ "$("$PIN" "$TMP/flake.lock" store-source revision)" == "$rev" ]]
[[ "$("$PIN" "$TMP/flake.lock" store-source url)" == https://github.com/example/store.git ]]
"$PIN" "$TMP/flake.lock" store-source json |
  jq -e --arg rev "$rev" \
    '.input == "store-source" and .repository == "example/store" and .rev == $rev and .narHash == "sha256-YWJjZA=="' \
    >/dev/null

for invalid in type owner revision hash; do
  case "$invalid" in
    type) write_lock store-source git example store "$rev" 'sha256-YWJjZA==' ;;
    owner) write_lock store-source github 'example;touch-pwned' store "$rev" 'sha256-YWJjZA==' ;;
    revision) write_lock store-source github example store main 'sha256-YWJjZA==' ;;
    hash) write_lock store-source github example store "$rev" 'not-a-hash' ;;
  esac
  if "$PIN" "$TMP/flake.lock" store-source json >"$TMP/invalid.out" 2>&1; then
    echo "github-flake-input-pin test: accepted invalid $invalid" >&2
    exit 1
  fi
done

for invalid_root in missing follows-array missing-node unsafe-input; do
  case "$invalid_root" in
    missing)
      printf '{"nodes":{"root":{"inputs":{}}}}\n' >"$TMP/flake.lock"
      ;;
    follows-array)
      printf '{"nodes":{"root":{"inputs":{"store-source":["parent","store-source"]}}}}\n' >"$TMP/flake.lock"
      ;;
    missing-node)
      printf '{"nodes":{"root":{"inputs":{"store-source":"absent"}}}}\n' >"$TMP/flake.lock"
      ;;
    unsafe-input)
      write_lock store-source github example store "$rev" 'sha256-YWJjZA=='
      ;;
  esac
  input=store-source
  [[ "$invalid_root" == unsafe-input ]] && input='store-source] | .evil'
  if "$PIN" "$TMP/flake.lock" "$input" repository >"$TMP/invalid-root.out" 2>"$TMP/invalid-root.err"; then
    echo "github-flake-input-pin test: accepted invalid root/input shape '$invalid_root'" >&2
    exit 1
  fi
  [[ ! -s "$TMP/invalid-root.out" ]]
  [[ -s "$TMP/invalid-root.err" ]]
done

# Beagle is a source-only test input. North's package graph must not regain the
# engine's runtime closure merely to keep its integration tests content-addressed.
input=beagle-engine-source
current_repository="$("$PIN" "$ROOT/flake.lock" "$input" repository)"
current_revision="$("$PIN" "$ROOT/flake.lock" "$input" revision)"
[[ "$current_repository" == tompassarelli/beagle ]]
[[ "$current_revision" == 12897f67848582f34aa61236ef4ce1252769d914 ]]
[[ "$current_repository" == "$(jq -r --arg input "$input" '.nodes[.nodes.root.inputs[$input]].locked | .owner + "/" + .repo' "$ROOT/flake.lock")" ]]
[[ "$current_revision" == "$(jq -r --arg input "$input" '.nodes[.nodes.root.inputs[$input]].locked.rev' "$ROOT/flake.lock")" ]]
[[ "$(jq -r --arg input "$input" '.nodes[.nodes.root.inputs[$input]].flake' "$ROOT/flake.lock")" == false ]]

echo "github flake input pin tests: PASS"
