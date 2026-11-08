use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::*;

const NORTH_CHECKS: &str = "[landing]\n\tremote-hosts = gex45\n[land]\n\tafter = @enqueue,** :: CARGO_TARGET_DIR=target ./bin/head-north config agents check-owners --north-root $PWD --south-root $HOME/code/south/main --nixos-root $HOME/code/nixos-config/main # hardcoded-repo-path:allow\n";

fn tool(bin: &Path, name: &str, body: &str) {
    let path = bin.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn run(needs: &Needs, home: &Path, bin: &Path) -> Vec<String> {
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new("sh").arg("-c").arg(needs.script("cpx62")).env("HOME", home).env("PATH", path).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    missing(&String::from_utf8_lossy(&output.stdout))
}

// KEEP(bug): cpx62 entered offload on 2026-11-08 with only ~/code/north/main and ~/code/south/main and no gex45 alias, so south's post-land owner check called main red and kills checks ended "Could not resolve hostname gex45" (north#706)
#[test]
fn a_build_host_without_a_checkout_or_ssh_alias_the_land_checks_read_reports_each_missing() {
    let root = tempfile::tempdir().unwrap();
    let (home, bin) = (root.path().join("home"), root.path().join("bin"));
    for checkout in ["code/north/main/.git", "code/south/main/.git"] {
        std::fs::create_dir_all(home.join(checkout)).unwrap();
    }
    std::fs::create_dir_all(&bin).unwrap();
    tool(&bin, "ssh", "echo \"hostname $2\"");
    tool(&bin, "getent", "exit 2");
    let needs = Needs::of(NORTH_CHECKS);
    assert_eq!(run(&needs, &home, &bin), ["~/code/nixos-config/main", "the ssh alias gex45"]);

    std::fs::create_dir_all(home.join("code/nixos-config/main/.git")).unwrap();
    tool(&bin, "ssh", "echo 'hostname 10.78.0.2'");
    assert_eq!(run(&needs, &home, &bin), Vec::<String>::new(), "a host with every checkout and the alias is ready");
}

#[test]
fn a_build_host_that_dials_no_remote_host_needs_none_of_their_ssh_aliases() {
    let root = tempfile::tempdir().unwrap();
    let (home, bin) = (root.path().join("home"), root.path().join("bin"));
    for checkout in ["code/north/main/.git", "code/south/main/.git", "code/nixos-config/main/.git"] {
        std::fs::create_dir_all(home.join(checkout)).unwrap();
    }
    std::fs::create_dir_all(&bin).unwrap();
    tool(&bin, "ssh", "echo \"hostname $2\"");
    tool(&bin, "getent", "exit 2");
    let needs = Needs::of(NORTH_CHECKS);
    assert_eq!(run(&needs.dialed_from("cpx62", |_| false), &home, &bin), Vec::<String>::new(), "on 2026-11-08 at 7:20 AM cpx62, which reaches no other host, so a mutant kill in its slot runs there and dials no gex45, left the land checks for lacking the gex45 alias and the build slots fell from 17 to 10");
    assert_eq!(run(&needs.dialed_from("cpx62", |alias| alias == "gex45"), &home, &bin), ["the ssh alias gex45"]);
}
