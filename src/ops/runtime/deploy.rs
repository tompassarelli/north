use super::layout::{Layout, PROGRAMS};
use super::update::{Failure, built_main, checkout, code, failed, lock};
use rustix::fs::FlockOperation;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

const STORE: &[u8] = b"/nix/store/";
const HASH: usize = 32;

pub(super) fn deploy(layout: &Layout, hosts: &[String]) -> Result<(), Failure> {
    let _lock = lock(layout, FlockOperation::LockExclusive)?;
    let (target, runtime) = built_main(layout, &checkout())?;
    let references = store_references(&runtime)?;
    for host in hosts {
        ship(host, &target, &runtime, &references)?;
    }
    Ok(())
}

fn ship(host: &str, target: &str, runtime: &Path, references: &BTreeSet<String>) -> Result<(), Failure> {
    let mut realise = vec!["nix-store", "--realise", "--quiet"];
    realise.extend(references.iter().map(String::as_str));
    run(host, "nix-store --realise", ssh(host).args(&realise).stdout(std::process::Stdio::null()))?;
    let destination = format!("{host}:.local/state/north/runtimes/{target}/");
    let mut copy = Command::new("rsync");
    copy.args(["-a", "--mkpath"]).args(PROGRAMS.map(|program| runtime.join(program))).arg(destination);
    run(host, "rsync", &mut copy)?;
    run(host, "north runtime update", ssh(host).args(["north", "runtime", "update"]))
}

fn ssh(host: &str) -> Command {
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", host]);
    command
}

fn run(host: &str, step: &str, command: &mut Command) -> Result<(), Failure> {
    let status = command.status()?;
    if !status.success() {
        return Err(failed(code(status), format!("{step} on {host} failed: {status}")));
    }
    Ok(())
}

fn store_references(runtime: &Path) -> Result<BTreeSet<String>, Failure> {
    let mut references = BTreeSet::new();
    for program in PROGRAMS {
        references.extend(scan(&fs::read(runtime.join(program))?));
    }
    Ok(references)
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
