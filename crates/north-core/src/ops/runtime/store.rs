use super::failure::{Failure, failed};
use super::layout::PROGRAMS;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

const STORE: &[u8] = b"/nix/store/";
const HASH: usize = 32;
const ROOT_PREFIX: &str = "gcroot-";

pub(super) fn references(runtime: &Path) -> Result<BTreeSet<String>, Failure> {
    let mut references = BTreeSet::new();
    for program in PROGRAMS {
        references.extend(scan(&fs::read(runtime.join(program))?));
    }
    Ok(references)
}

pub(super) fn root(runtime: &Path) -> Result<(), Failure> {
    for reference in references(runtime)? {
        let name = reference.rsplit('/').next().unwrap_or_default();
        let status = Command::new("nix-store")
            .arg("--add-root")
            .arg(runtime.join(format!("{ROOT_PREFIX}{name}")))
            .args(["--indirect", "--realise", &reference])
            .stdout(Stdio::null())
            .status()
            .map_err(|error| failed(1, format!("nix-store did not run: {error}")))?;
        if !status.success() {
            return Err(failed(1, format!("rooting {reference} failed: {status}")));
        }
    }
    Ok(())
}

fn scan(bytes: &[u8]) -> Vec<String> {
    let named = |byte: &u8| byte.is_ascii_alphanumeric() || b"+-._?=".contains(byte);
    let hashed = |byte: &u8| matches!(byte, b'0'..=b'9' | b'a'..=b'z') && !b"eotu".contains(byte);
    let mut found = Vec::new();
    for (at, _) in bytes.windows(STORE.len()).enumerate().filter(|(_, window)| *window == STORE) {
        let rest = &bytes[at + STORE.len()..];
        if rest.len() > HASH && rest[..HASH].iter().all(hashed) && rest[HASH] == b'-' {
            let name = rest[HASH + 1..].iter().take_while(|byte| named(byte)).count();
            found.push(String::from_utf8_lossy(&bytes[at..at + STORE.len() + HASH + 1 + name]).into_owned());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    #[test]
    fn scan_finds_each_store_path_root_once_per_mention() {
        let hash = "n51dhmdbik1kfrsm62j5knavmigwrl1a";
        let bytes = format!("\0/nix/store/{hash}-glibc-2.42-84/lib/ld.so\0/nix/store/short-x\0/nix/store/{hash}-gcc-15.3.0-lib:");
        assert_eq!(super::scan(bytes.as_bytes()), [format!("/nix/store/{hash}-glibc-2.42-84"), format!("/nix/store/{hash}-gcc-15.3.0-lib")]);
    }
}
