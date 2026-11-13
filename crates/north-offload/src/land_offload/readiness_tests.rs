use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::*;

const NORTH_CHECKS: &str = "[landing]\n\tremote-hosts = gex45\n[land]\n\tafter = @enqueue,** :: CARGO_TARGET_DIR=target ./bin/head-north config agents check-owners --north-root $PWD --south-root $HOME/code/south/main --nixos-root $HOME/code/nixos-config/main # hardcoded-repo-path:allow\n\tcheck = src/cli/operator.rs,src/cli/operator/,crates/north-ops/src/ops/credentials/ :: CARGO_TARGET_DIR=target ./bin/head-north --code src/cli/operator/ config operator check $HOME/code/south/main # hardcoded-repo-path:allow\n";

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

#[test]
fn a_build_host_without_an_input_a_check_declares_is_left_out_of_that_check() {
    let root = tempfile::tempdir().unwrap();
    let (cpx62b, gex45, bin) = (root.path().join("cpx62b"), root.path().join("gex45"), root.path().join("bin"));
    for home in [&cpx62b, &gex45] {
        std::fs::create_dir_all(home.join("code/north/main/.git")).unwrap();
    }
    for input in [".local/state/north/graph.db", ".local/share/makgora-build-inputs/store"] {
        std::fs::create_dir_all(gex45.join(input)).unwrap();
    }
    std::fs::create_dir_all(&bin).unwrap();
    let graph = super::super::Placement::parse(&["--needs", "~/.local/state/north/graph.db"]).unwrap();
    let map = super::super::Placement::parse(&["--needs", "~/.local/share/makgora-build-inputs/store"]).unwrap();
    let (graph, map) = (Needs::default().with_inputs(&graph.needs), Needs::default().with_inputs(&map.needs));
    assert_eq!(run(&graph, &cpx62b, &bin), ["~/.local/state/north/graph.db"], "the graph guards line never goes to a host without graph.db");
    assert_eq!(run(&map, &cpx62b, &bin), ["~/.local/share/makgora-build-inputs/store"], "makgora's map-build line never goes to a host without its input store");
    assert_eq!(run(&graph, &gex45, &bin), Vec::<String>::new());
    assert_eq!(run(&map, &gex45, &bin), Vec::<String>::new());
    const STORE: &str = ".local/share/makgora-build-inputs/store";
    const IMPACT: &str = "impact-assets/14fbf7845b5d9c9705d0f0b4b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0";
    const BASE: &str = "base/775f8957e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0";
    let (cpx62, cpx62b) = (root.path().join("cpx62"), root.path().join("cpx62b"));
    for (home, entry) in [(&cpx62, IMPACT), (&cpx62b, BASE)] {
        std::fs::create_dir_all(home.join("code/north/main/.git")).unwrap();
        std::fs::create_dir_all(home.join(STORE).join(entry)).unwrap();
    }
    let entry = format!("~/{STORE}/{IMPACT}");
    let pinned = super::super::Placement::parse(&["--needs", &entry]).unwrap();
    let pinned = Needs::default().with_inputs(&pinned.needs);
    assert_eq!(run(&pinned, &cpx62b, &bin), [entry], "2cdd2aed1's map build was reverted on cpx62b, whose store existed but lacked its impact-assets entry");
    assert_eq!(run(&pinned, &cpx62, &bin), Vec::<String>::new(), "the store entry the check pins admits the host that holds it");
    assert_eq!(run(&Needs::default(), &cpx62b, &bin), Vec::<String>::new(), "a check that declares no input takes any host");
}

#[test]
fn a_host_missing_the_landing_repositorys_own_checkout_is_left_out_of_offload_choose() {
    let root = tempfile::tempdir().unwrap();
    let (laptop, cpx62, bin) = (root.path().join("laptop"), root.path().join("cpx62"), root.path().join("bin"));
    let wisp = laptop.join("code/wisp/main");
    std::fs::create_dir_all(&wisp).unwrap();
    assert!(Command::new("git").arg("init").arg("-q").arg(&wisp).status().unwrap().success());
    let lane = laptop.join("code/wisp/worktrees/lane");
    assert!(Command::new("git").current_dir(&wisp).args(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "one"]).status().unwrap().success());
    assert!(Command::new("git").current_dir(&wisp).args(["worktree", "add", "-q"]).arg(&lane).status().unwrap().success());
    std::fs::write(lane.join(".safe-push"), "[land]\n\tafter = ** :: bun install --frozen-lockfile --silent && bun wisp ci pre-merge --base ${SAFE_PUSH_LAND_BASE}\n").unwrap();
    for checkout in ["code/north/main/.git", "code/south/main/.git"] {
        std::fs::create_dir_all(cpx62.join(checkout)).unwrap();
    }
    std::fs::create_dir_all(&bin).unwrap();
    let needs = Needs::at(&lane, &laptop);
    assert_eq!(run(&needs, &cpx62, &bin), ["~/code/wisp/main"], "on 2026-11-08 at about 7:24 AM wisp's after-landing run of acd3c88de4 went to cpx62, which held north and south but no ~/code/wisp/main, and exited 75 three times");
    std::fs::create_dir_all(cpx62.join("code/wisp/main/.git")).unwrap();
    assert_eq!(run(&needs, &cpx62, &bin), Vec::<String>::new(), "a host holding the landing repository's own checkout takes its land checks");
}
