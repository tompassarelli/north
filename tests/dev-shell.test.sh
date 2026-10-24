#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "${scratch:?}"' EXIT
mkdir -p "$scratch/lane/src" "$scratch/lane/tests"
printf '[package]\nname = "probe"\nversion = "0.0.0"\nedition = "2024"\n' >"$scratch/lane/Cargo.toml"
printf 'fn main() { print!("{}", env!("CARGO_MANIFEST_DIR")); }\n' >"$scratch/lane/src/main.rs"
cat >"$scratch/lane/tests/here.rs" <<'EOF'
#[test]
fn runs_what_this_checkout_built() {
    let here = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let binary = std::process::Command::new(env!("CARGO_BIN_EXE_probe")).output().unwrap().stdout;
    assert_eq!((env!("CARGO_MANIFEST_DIR"), String::from_utf8(binary).unwrap().as_str()), (here.as_str(), here.as_str()));
}
EOF
git -C "$scratch/lane" init -q
build() {
  (cd "$scratch/$1" && CARGO_TARGET_DIR="$scratch/target" nix develop "$root" -c cargo test --offline -q 2>&1)
}
build lane
cp -R "$scratch/lane" "$scratch/scratch-copy"
build scratch-copy
build lane || {
  printf "dev shell: the lane ran a test binary another checkout built into its target\n" >&2
  exit 1
}
printf 'dev shell: a build from another checkout into a shared target is not run as fresh\n'
