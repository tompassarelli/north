use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, Ordering};

use super::catalog_init::catalog_from;
use super::owner_modules::STRUCTURE_FILE;
use super::registration_checks::{OwnerCheckRoots, canonical, check_owner_files, git_text};
use crate::error::NorthResult;

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid"])
        .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null"])
        .args(arguments)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("git output").trim().to_owned()
}

fn south_checkout(scratch: &Path) -> std::path::PathBuf {
    let origin = scratch.join("south-origin.git");
    let publisher = scratch.join("south-publisher");
    let south = scratch.join("south");
    git(scratch, &["init", "-q", "--bare", "-b", "main", origin.to_str().expect("utf-8 path")]);
    git(scratch, &["clone", "-q", origin.to_str().expect("utf-8 path"), publisher.to_str().expect("utf-8 path")]);
    std::fs::create_dir_all(publisher.join("agents")).expect("agents dir");
    std::fs::write(publisher.join("agents/AGENTS.md"), "rules").expect("rules");
    git(&publisher, &["add", "."]);
    git(&publisher, &["commit", "-q", "-m", "rules"]);
    git(&publisher, &["push", "-q", "origin", "HEAD:main"]);
    git(scratch, &["clone", "-q", origin.to_str().expect("utf-8 path"), south.to_str().expect("utf-8 path")]);
    south
}

fn checked_init(structure: &str) -> NorthResult<()> {
    let scratch = tempfile::tempdir().expect("scratch");
    let north = scratch.path().join("north");
    std::fs::create_dir_all(north.join("harness/skills/a")).expect("skill dir");
    std::fs::write(north.join("harness/skills/a/SKILL.md"), "a").expect("skill");
    std::fs::write(north.join(STRUCTURE_FILE), structure).expect("structure");
    git(scratch.path(), &["init", "-q", "-b", "main", north.to_str().expect("utf-8")]);
    git(&north, &["add", "."]);
    git(&north, &["commit", "-q", "-m", "fixture"]);
    let mut catalog = catalog_from("[[registered]]\nid = \"skill.a\"\npath = \"harness/skills/a/SKILL.md\"\n", "")?;
    let mut keys: Vec<_> = catalog.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["$schema", "activation", "baselines", "providerSupport", "registrations", "role", "rootOrder", "schema"]);
    let roots = OwnerCheckRoots { north: north.clone(), south: south_checkout(scratch.path()), nixos: None };
    let head = git_text(&north, &["rev-parse", "HEAD"])?;
    check_owner_files(&mut catalog, &roots, &canonical(&north)?, &head)
}

const CHURNERS: usize = 4;

#[test]
fn catalog_init_resolves_and_refuses_an_unregistered_module_and_concurrent_owner_checks_fetch_one_checkout_without_racing_its_refs() {
    checked_init("[[registered]]\nid = \"skill.a\"\npath = \"harness/skills/a/SKILL.md\"\n").expect("resolves");
    let error = checked_init("").expect_err("unregistered");
    assert!(error.to_string().contains("does not register skill.a"), "{error}");
    let scratch = tempfile::tempdir().expect("scratch");
    let origin = scratch.path().join("origin.git");
    let publisher = scratch.path().join("publisher");
    let nixos = scratch.path().join("nixos");
    git(scratch.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().expect("utf-8 path")]);
    git(scratch.path(), &["clone", "-q", origin.to_str().expect("utf-8 path"), publisher.to_str().expect("utf-8 path")]);
    git(&publisher, &["commit", "-q", "--allow-empty", "-m", "first"]);
    git(&publisher, &["push", "-q", "origin", "HEAD:main"]);
    git(scratch.path(), &["clone", "-q", origin.to_str().expect("utf-8 path"), nixos.to_str().expect("utf-8 path")]);
    let lanes: String = (0..300).map(|lane| format!("create refs/heads/lane-{lane} HEAD\n")).collect();
    let mut seed = Command::new("git").arg("-C").arg(&nixos).args(["update-ref", "--stdin"]).stdin(Stdio::piped()).spawn().expect("update-ref runs");
    seed.stdin.take().expect("update-ref stdin").write_all(lanes.as_bytes()).expect("lanes written");
    assert!(seed.wait().expect("update-ref exits").success(), "lanes seeded");
    let roots = OwnerCheckRoots { north: scratch.path().join("north"), south: scratch.path().join("south"), nixos: Some(nixos) };
    for round in 0..12 {
        git(&publisher, &["commit", "-q", "--allow-empty", "-m", &format!("round {round}")]);
        git(&publisher, &["push", "-q", "origin", "HEAD:main"]);
        let tip = git(&publisher, &["rev-parse", "HEAD"]);
        let start = Barrier::new(2 + CHURNERS);
        let checking = AtomicBool::new(true);
        let resolved: Vec<_> = std::thread::scope(|scope| {
            for churner in 0..CHURNERS {
                let (start, checking, checkout) = (&start, &checking, roots.nixos.as_ref().expect("nixos root"));
                scope.spawn(move || {
                    start.wait();
                    let mut lane = 0;
                    while checking.load(Ordering::Relaxed) {
                        let branch = format!("refs/heads/churn-{churner}-{lane}");
                        let _ = Command::new("git").arg("-C").arg(checkout).args(["update-ref", &branch, "HEAD"]).status();
                        let _ = Command::new("git").arg("-C").arg(checkout).args(["update-ref", "-d", &branch]).status();
                        lane += 1;
                    }
                });
            }
            let runs: Vec<_> = (0..2)
                .map(|_| scope.spawn(|| {
                    start.wait();
                    roots.resolve("nixos-config", "/elsewhere", "unused").map(|(_, reference)| reference).map_err(|error| error.to_string())
                }))
                .collect();
            let resolved = runs.into_iter().map(|run| run.join().expect("owner check thread")).collect();
            checking.store(false, Ordering::Relaxed);
            resolved
        });
        assert_eq!(resolved, vec![Ok(tip.clone()), Ok(tip)], "round {round}");
    }
}
