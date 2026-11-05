use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, Ordering};

use super::registration_checks::OwnerCheckRoots;

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
