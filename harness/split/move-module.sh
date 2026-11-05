#!/usr/bin/env bash
set -euo pipefail
module=${1:?usage: move-module.sh MODULE CRATE}
crate=${2:?usage: move-module.sh MODULE CRATE}
ident=${crate//-/_}
core=crates/north-core
cd "$(git rev-parse --show-toplevel)"
dest=crates/$crate/src
mkdir -p "$dest"
for src in "$core/src/$module" "$core/src/$module.rs"; do if [ -e "$src" ]; then git mv "$src" "$dest/"; fi; done
moved=$dest/$module; [ -e "$moved" ] || moved=$dest/$module.rs
sed -i "/^pub mod $module;$/d" "$core/src/lib.rs"

files=$(find $moved -name '*.rs' | sort)
part() { for f in $files; do case $f in */tests.rs|*_tests.rs) [ "$1" != test ] || cat "$f" ;; *)
  awk -v w="$1" 'p ~ /^#\[cfg\(test\)\]/ && /^mod [a-z_0-9]+ *\{/ {t=1} t == (w == "test") {print} {p=$0}' "$f" ;; esac; done; }
uses() { rg -q -e "(^|[^:\w])$1::" -e "use $1\b" -e "(^|[^:\w])$1!" <<<"$2"; }
prod=$(part prod); tests=$(part test)

reached=$(cat $files | rg -o 'crate::(\{[^}]*\}|[a-z_0-9]+)' | sed 's/^crate:://; s/[{}]//g' | tr ',' '\n' \
  | sed 's/::.*//; s/^ *//; s/ *$//' | rg -v "^($module|self|super)?$" | sort -u | paste -sd, - | sed 's/,/, /g')

workspace=$(sed -n '/^\[workspace.dependencies\]/,/^\[/p' Cargo.toml | sed -n 's/^\([A-Za-z0-9_-]*\) *=.*/\1/p')
metadata=$(cargo metadata --format-version 1 --offline)
lib_of() { jq -r --arg p "$1" '[.packages[] | select(.name == $p) | .targets[] | select(.kind | index("lib")) | .name][0] // ($p | gsub("-"; "_"))' <<<"$metadata"; }
deps=; dev=
for name in $workspace; do
  lib=$(lib_of "$name")
  if uses "$lib" "$prod"; then deps+="$name.workspace = true"$'\n'; elif uses "$lib" "$tests"; then dev+="$name.workspace = true"$'\n'; fi
done
while IFS= read -r line; do
  name=${line%% *}; if uses "$(lib_of "$name")" "$tests"; then dev+="$line"$'\n'; fi
done < <(sed -n '/^\[dev-dependencies\]/,/^\[/p' "$core/Cargo.toml" | rg '^[A-Za-z0-9_-]+ *=')

{ sed -n '/^\[package\]/,/^$/p' "$core/Cargo.toml" | sed "s/^name = .*/name = \"$crate\"/"
  printf '[dependencies]\nnorth-core = { path = "../north-core" }\n%s\n' "$deps"
  [ -z "$dev" ] || printf '[dev-dependencies]\n%s\n' "$dev"
  printf '[lints]\nworkspace = true\n'; } > "crates/$crate/Cargo.toml"
printf 'pub use north_core::{%s};\n\npub mod %s;\n' "$reached" "$module" > "$dest/lib.rs"

rg -q "\"crates/$crate\"" Cargo.toml || sed -i -E "s|^((default-)?members = \[.*)\]$|\1, \"crates/$crate\"]|" Cargo.toml
rg -q "^$crate = " Cargo.toml || sed -i "s|^north-core = { path = \"crates/north-core\" }$|&\n$crate = { path = \"crates/$crate\" }|" Cargo.toml
for main in $(rg -l "use north_core::\{[^}]*\b$module\b" --multiline src); do
  perl -0pi -e "s/(use north_core::\{[^}]*?)\b$module,\s*/\$1/s; s/(use north_core::\{[^}]*?),\s*$module\b/\$1/s" "$main"
  perl -0pi -e "s/\A(.*^use north_[^;]*;\n)/\$1use $ident::$module;\n/ms" "$main"
done
{ rg -l "north_core::$module::" src tests crates || true; } | xargs -r sed -i "s/north_core::$module::/$ident::$module::/g"
{ git ls-files -z | xargs -0 rg -l "$core/src/$module\b" || true; } | { rg -v "^$dest/" || true; } \
  | xargs -r sed -i -E "s#$core/src/$module([/.\`\"*]|$)#crates/$crate/src/$module\1#g"

roots=$(for name in ${reached//,/ }; do awk -v want="/src/$name" '/^name = /{n=$3} /^paths = /&&(index($0, want"/**\"")||index($0, want".rs\"")){gsub(/"/,"",n); print n}' structure.toml; done | sort -u)
for name in $roots; do
  awk -v add="$name" '/^name = "crate-roots"/{r=1} r&&/^may_use = /&&!index($0,"\""add"\""){sub(/\]$/, ", \""add"\"]"); r=0} {print}' structure.toml > structure.toml.new && mv structure.toml.new structure.toml
done

for _ in 1 2 3 4 5 6 7 8; do
  json=$(cargo check -p "$crate" --all-targets --offline --message-format=json 2>/dev/null || true)
  spans=$(jq -r 'select(.reason == "compiler-message") | .message | select(.code.code // "" | test("E0603|E0624|E0432|E0433|E0425|E0412|private_interfaces"))
      | (.spans[], .children[].spans[]) | select(.file_name | test("^crates/north-(core|base)/")) | "\(.file_name):\(.line_start)"' <<<"$json" | sort -u)
  fields=$(jq -r 'select(.reason == "compiler-message") | .message | select(.code.code == "E0616") | .message' <<<"$json" \
    | sed -n -E 's/^field `([a-z_0-9]+)` of struct `north_(core|base)::(.*)::([A-Za-z0-9_]+)` is private$/\2 \3 \4 \1/p')
  while read -r owner path struct field; do
    [ -n "$owner" ] || continue
    for file in "crates/north-$owner/src/${path//:://}.rs" "crates/north-$owner/src/${path//:://}/mod.rs"; do
      [ -e "$file" ] || continue
      spans+=$'\n'$(awk -v s="$struct" -v f="$field" -v at="$file" '$0 ~ "struct "s" \\{" {i=1} i && $0 ~ "^ *(pub\\([^)]*\\) )?"f":" {print at":"NR; i=0} i && /^}/ {i=0}' "$file")
    done
  done <<<"$fields"
  spans=$(rg -v '^$' <<<"$spans" || true)
  [ -z "$spans" ] && break
  for at in $spans; do
    sed -i -E "${at##*:}{s/^( *)#\[cfg\(test\)\]$/\1#[doc(hidden)]/; t; s/\bpub\((crate|super|in [^)]*)\) /pub /; t; s/^( *)((async |const |unsafe )*(fn|struct|enum|type|trait|const|static|mod|use) |[a-z_0-9]+: )/\1pub \2/}" "${at%:*}"
  done
done
cargo check -p "$crate" --all-targets --offline
