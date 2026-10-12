#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$repo/harness/scripts/rust-build-timings.sh"
file=Cargo.toml
before="$(sha256sum "$repo/$file")"
out="$("$script" --dry-run --label base "$repo" "$file")"
[ "$(sha256sum "$repo/$file")" = "$before" ]
steps="$(cut -f1 <<<"$out" | paste -sd' ')"
[ "$steps" = "shell clean-build touch incremental-build test-no-run test-run" ] || { printf 'steps: %s\n' "$steps" >&2; exit 1; }
dirs="$(grep -o 'CARGO_TARGET_DIR=[^ ]*' <<<"$out" | sort -u)"
[ "$(wc -l <<<"$dirs")" = 1 ] || { printf 'target dirs:\n%s\n' "$dirs" >&2; exit 1; }
[ ! -e "${dirs#CARGO_TARGET_DIR=}" ]
grep -q "^touch.*>> $repo/$file\$" <<<"$out"
! "$script" --dry-run "$repo" no-such-file 2>/dev/null
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/bin" "$scratch/crate/src"
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$CARGO_TARGET_DIR" >>"%s/targets"\n' "$scratch" >"$scratch/bin/cargo"
chmod +x "$scratch/bin/cargo"
printf 'pub fn f() {}\n' >"$scratch/crate/src/lib.rs"
lib_before="$(sha256sum "$scratch/crate/src/lib.rs")"
rows="$(PATH="$scratch/bin:$PATH" RUST_BUILD_TIMINGS_SHELL=1 "$script" --label stub "$scratch/crate" src/lib.rs 2>/dev/null)"
[ "$(sha256sum "$scratch/crate/src/lib.rs")" = "$lib_before" ] || { printf 'touched file not restored\n' >&2; exit 1; }
[ "$(cut -f1,2 <<<"$rows" | paste -sd' ')" = "stub	clean-build stub	incremental-build stub	test-no-run stub	test-run" ] || { printf 'rows: %s\n' "$rows" >&2; exit 1; }
[ "$(sort -u "$scratch/targets" | wc -l)" = 1 ] && [ ! -e "$(head -1 "$scratch/targets")" ]
printf 'ok: dry run plans one fresh target dir, touches the file between builds and leaves it unchanged; a stubbed real run restores the touched file and prints one row per step\n'
