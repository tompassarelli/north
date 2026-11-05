const TOOLCHAIN_MISSING: u8 = 75;

const CARGO_TEST: &str = "cargo test --locked --offline";
const CARGO_CLIPPY: &str = "cargo clippy --all-targets --locked --offline -- -D warnings";
const BUN_TEST: &str = "bun test";
const FLAKE_CHECK: &str = "nix flake check --offline --no-update-lock-file";

fn has_file(files: &[String], name: &str) -> bool {
    files.iter().any(|file| file == name)
}

fn identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'\'')
}

fn names_attribute(source: &str, name: &str) -> bool {
    source.lines().map(|line| line.split('#').next().unwrap_or_default()).any(|line| {
        let bytes = line.as_bytes();
        line.match_indices(name).any(|(start, _)| {
            let before = start.checked_sub(1).and_then(|index| bytes.get(index));
            let after = bytes.get(start + name.len());
            !before.is_some_and(|byte| identifier_byte(*byte)) && !after.is_some_and(|byte| identifier_byte(*byte))
        })
    })
}

fn guarded(tool: &str, command: &str, dev_shell: bool) -> String {
    let guard = format!("command -v {tool} >/dev/null || exit {TOOLCHAIN_MISSING}; {command}");
    if dev_shell {
        format!("command -v nix >/dev/null || exit {TOOLCHAIN_MISSING}; nix develop --offline --no-update-lock-file -c bash -euc '{guard}'")
    } else {
        guard
    }
}

pub fn dev_shell(flake: &str) -> bool {
    names_attribute(flake, "devShells") || names_attribute(flake, "devShell")
}

pub fn infer(files: &[String], flake: Option<&str>) -> Vec<String> {
    let flake = if has_file(files, "flake.nix") { flake } else { None };
    let dev_shell = flake.is_some_and(dev_shell);
    let mut checks = Vec::new();
    if has_file(files, "Cargo.toml") {
        checks.push(guarded("cargo", CARGO_TEST, dev_shell));
        checks.push(guarded("cargo", CARGO_CLIPPY, dev_shell));
    }
    if has_file(files, "package.json") && has_file(files, "bun.lock") {
        checks.push(guarded("bun", BUN_TEST, dev_shell));
    }
    if flake.is_some_and(|source| names_attribute(source, "checks")) {
        checks.push(guarded("nix", FLAKE_CHECK, false));
    }
    checks
}

#[cfg(test)]
#[path = "infer_tests.rs"]
mod tests;
