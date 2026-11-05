use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

use super::owner_modules::resolve_owner_modules;
use super::registration_checks::{OwnerCheckRoots, check_owner_files};

const STRUCTURE: &str = "[[registered]]\nid = \"guard.sample\"\npath = \"src/guard/sample.rs\"\n";

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

fn catalog() -> Value {
    json!({"registrations": {"sample-guard": {
        "kind": "hook",
        "owner": {"repo": "north", "module": "guard.sample"}
    }}})
}

fn resolve_against(structure: &str) -> Result<Value, String> {
    let mut value = catalog();
    resolve_owner_modules(&mut value, &mut |_| Ok(structure.to_owned()))
        .map(|()| value)
        .map_err(|error| error.to_string())
}

#[test]
fn module_owner_resolves_to_its_registered_path() {
    let value = resolve_against(STRUCTURE).expect("registered module resolves");
    assert_eq!(
        value["registrations"]["sample-guard"]["owner"],
        json!({"repo": "north", "path": "src/guard/sample.rs"})
    );
}

#[test]
fn unregistered_or_ambiguous_module_is_refused() {
    let missing = resolve_against("[[registered]]\nid = \"guard.other\"\npath = \"x\"\n")
        .expect_err("unregistered module refuses");
    assert!(missing.contains("does not register guard.sample"), "{missing}");
    let doubled = resolve_against(&STRUCTURE.repeat(2)).expect_err("duplicate id refuses");
    assert!(doubled.contains("more than once"), "{doubled}");
}

#[test]
fn moving_a_registered_guard_keeps_the_land_check_passing_without_a_catalog_change() {
    let scratch = tempfile::tempdir().expect("scratch");
    let north = scratch.path().join("north");
    let south = scratch.path().join("south");
    fs::create_dir_all(north.join("src/guard")).expect("north tree");
    fs::create_dir_all(&south).expect("south tree");
    git(&north, &["init", "-q", "-b", "main"]);
    fs::write(north.join("structure.toml"), STRUCTURE).expect("structure");
    fs::write(north.join("src/guard/sample.rs"), "// guard\n").expect("guard source");
    git(&north, &["add", "-A"]);
    git(&north, &["commit", "-qm", "register guard"]);
    let roots = OwnerCheckRoots {
        north: north.clone(),
        south,
        nixos: scratch.path().join("nixos"),
    };
    let root = fs::canonicalize(&north).expect("canonical north").display().to_string();
    let check = || check_owner_files(&mut catalog(), &roots, &root, &git(&north, &["rev-parse", "HEAD"]));

    check().expect("registered guard resolves before the move");

    fs::create_dir_all(north.join("crates/core")).expect("new home");
    git(&north, &["mv", "src/guard/sample.rs", "crates/core/sample.rs"]);
    git(&north, &["commit", "-qm", "move guard without updating structure.toml"]);
    let refused = check().expect_err("a moved file without its structure.toml entry refuses");
    assert!(refused.to_string().contains("src/guard/sample.rs"), "{refused}");

    fs::write(
        north.join("structure.toml"),
        STRUCTURE.replace("src/guard/sample.rs", "crates/core/sample.rs"),
    )
    .expect("structure move");
    git(&north, &["commit", "-qam", "move the registration with the file"]);
    check().expect("the same catalog resolves after the move");
}

const CHURNERS: usize = 4;

#[test]
fn concurrent_owner_checks_fetch_one_checkout_without_racing_its_refs() {
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
    let roots = OwnerCheckRoots { north: scratch.path().join("north"), south: scratch.path().join("south"), nixos };
    for round in 0..12 {
        git(&publisher, &["commit", "-q", "--allow-empty", "-m", &format!("round {round}")]);
        git(&publisher, &["push", "-q", "origin", "HEAD:main"]);
        let tip = git(&publisher, &["rev-parse", "HEAD"]);
        let start = Barrier::new(2 + CHURNERS);
        let checking = AtomicBool::new(true);
        let resolved: Vec<_> = std::thread::scope(|scope| {
            for churner in 0..CHURNERS {
                let (start, checking, checkout) = (&start, &checking, &roots.nixos);
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
