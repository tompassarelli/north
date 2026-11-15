use north_foundation::test_inputs::git::git_in as git;
use std::path::Path;

use north_foundation::foundation::shim::write_script;
use north_offload::check_offload::offloaded;

use super::super::{Lane, Road};
use super::{passes_placed, verdict};

struct Laptop(String);

impl Road for Laptop {
    fn north(&self) -> String {
        self.0.clone()
    }

    fn take(self, _: &Lane) -> u8 {
        0
    }
}

fn stubs(root: &Path, north: &str) -> (String, String, std::path::PathBuf) {
    let (cargo, north_stub, local) = (root.join("cargo"), root.join("north"), root.join("local"));
    write_script(&cargo, &format!("touch '{}'\n", local.display()));
    write_script(&north_stub, north);
    (cargo.display().to_string(), north_stub.display().to_string(), local)
}

#[test]
fn a_land_check_on_a_non_build_host_spawns_no_local_rustc() {
    let root = tempfile::tempdir().unwrap();
    let (cargo, north, local) = stubs(root.path(), &format!("printf '%s\\n' \"$@\" > '{}'\n", root.path().join("asked").display()));
    let check = format!("{cargo} nextest run -p north-capacity");
    let placed = offloaded(&north, &check, ("base", "head"), false, || Some("gex45".to_owned()));
    assert_eq!(passes_placed(root.path(), &check, placed, &[]), Ok(()));
    assert!(!local.exists(), "on 2026-11-14 a check that compiles ran its cargo on the laptop, which froze it at load 51");
    let asked = std::fs::read_to_string(root.path().join("asked")).unwrap();
    assert!(asked.contains("--host\ngex45\n--base\nbase\n--head\nhead\n") && asked.contains(&check), "{asked}");
    assert_eq!(passes_placed(root.path(), &check, offloaded(&north, &check, ("base", "head"), true, || None), &[]), Ok(()));
    assert!(local.exists(), "a build host runs the check where it stands");
}

#[test]
fn a_finish_call_check_from_a_laptop_lane_runs_on_a_build_host() {
    let root = tempfile::tempdir().unwrap();
    let (cargo, north, local) = stubs(root.path(), "echo 'tests failed on the build host'; exit 3\n");
    let check = format!("{cargo} test");
    let placed = offloaded(&north, &check, ("base", "head"), false, || Some("cpx62".to_owned()));
    let (failure, output) = passes_placed(root.path(), &check, placed, &[]).unwrap_err();
    assert_eq!((failure.as_str(), output.contains("tests failed on the build host")), ("exited 3", true), "the build host's result comes back as the check's own");
    let (origin, lane) = (root.path().join("origin.git"), root.path().join("lane"));
    std::fs::create_dir_all(&lane).unwrap();
    git(&lane, &["init", "-q", "-b", "main"]);
    git(root.path(), &["init", "-q", "--bare", "-b", "main", &origin.display().to_string()]);
    git(&lane, &["remote", "add", "origin", &origin.display().to_string()]);
    git(&lane, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(&lane, &["push", "-q", "origin", "main"]);
    let head = git(&lane, &["rev-parse", "HEAD"]);
    let lane = Lane { root: lane, destination: "main".to_owned(), message: String::new() };
    let silent = root.path().join("silent");
    write_script(&silent, "exit 1\n");
    let (checked, ran) = verdict(&Laptop(silent.display().to_string()), &lane, Some((&check, &head)), Some(&check), None);
    assert_eq!((checked.unwrap_err().0, ran), ("exited 75".to_owned(), false), "with no build host the check is refused, never run on the laptop");
    assert!(!local.exists());
}

#[test]
fn a_landing_verbs_lane_check_runs_under_the_sessions_confinement() {
    let root = tempfile::tempdir().unwrap();
    let (asked, ran) = (root.path().join("asked"), root.path().join("ran"));
    let north = root.path().join("north");
    write_script(&north, &format!("printf '%s\\n' \"$@\" > '{}'\nshift 3\nexec \"$@\"\n", asked.display()));
    let lane = Lane { root: root.path().to_owned(), destination: "main".to_owned(), message: String::new() };
    let check = format!("touch '{}'", ran.display());
    let (checked, recorded) = verdict(&Laptop(north.display().to_string()), &lane, None, Some(&check), None);
    assert_eq!((checked, recorded), (Ok(()), false));
    let asked = std::fs::read_to_string(&asked).unwrap_or_default();
    assert!(asked.starts_with(&format!("--worker-sandbox\n--program\n--\nsh\n-c\n{check}\n")), "a finish call's own check did not start under the session's confinement (north#1397): {asked:?}");
    assert!(ran.exists());
}
