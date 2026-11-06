use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Output;

use super::{Fixture, executable};

const SYSTEMCTL: &str = "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>\"$LAND_TRACE/systemctl\"\n";

fn runtime(fixture: &Fixture, name: &str, binary: Option<&str>) -> PathBuf {
    let bin = fixture.state.join("north/runtimes").join(name).join("tree/bin");
    fs::create_dir_all(&bin).unwrap();
    match binary {
        Some(body) => executable(&bin.join("north"), body),
        None => symlink(env!("CARGO_BIN_EXE_north"), bin.join("north")).unwrap(),
    }
    PathBuf::from("runtimes").join(name)
}

fn point(fixture: &Fixture, link: &str, target: &Path) {
    let link = fixture.state.join("north").join(link);
    let _ = fs::remove_file(&link);
    symlink(target, link).unwrap();
}

fn lander_link(fixture: &Fixture) -> Option<PathBuf> {
    fs::read_link(fixture.state.join("north/lander")).ok()
}

fn promote(fixture: &Fixture, extra: &[&str]) -> (Output, String) {
    let output = fixture.north().args(["land", "promote", "--shadow-minutes", "0", "--repo", fixture.repo.to_str().unwrap(), "--checker", fixture.trace.join("checker").to_str().unwrap()]).args(extra).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    eprintln!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
    (output, stdout)
}

fn served() -> Fixture {
    let mut fixture = Fixture::new();
    executable(&fixture.bin.join("systemctl"), SYSTEMCTL);
    let lanes: Vec<PathBuf> = [("a", "a.txt", "a\n"), ("b", "b.txt", "b\n"), ("c", "app.txt", "ALPHA\nbeta\ngamma\n"), ("d", "app.txt", "alpha\nBETA\ngamma\n")].iter().map(|(name, file, text)| fixture.lane(name, file, text)).collect();
    for lane in &lanes {
        fixture.enqueue(lane, None);
    }
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c"]);
    fixture
}

fn replaying(failing: &[&str], verdict: &str) -> String {
    let lines: String = failing.iter().map(|test| format!("echo 'replay: failure cargo {test}'; ")).collect();
    format!("#!/usr/bin/env bash\nif [ \"$1 $2\" = \"land replay\" ]; then {lines}echo 'replay: verdict {verdict}'; exit 0; fi\nexec {} \"$@\"\n", env!("CARGO_BIN_EXE_north"))
}

#[test]
fn promote_refuses_a_candidate_with_a_new_failure_and_keeps_the_lander_link_but_follows_one_with_none() {
    let fixture = served();
    let good = runtime(&fixture, "good", None);
    point(&fixture, "lander", &good);

    let drifted = runtime(&fixture, "drifted", Some(&replaying(&["lander_probe"], "failed drifted judgement")));
    point(&fixture, "current", &drifted);
    let (output, stdout) = promote(&fixture, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("refused: round lane-") && stdout.contains("but the candidate replayed failed drifted judgement") && stdout.contains("the held runtime replayed landed") && stdout.contains("the lander link is unchanged"), "{stdout}");
    assert!(!stdout.contains("shadow:"), "a replay mismatch refuses before the shadow");
    assert_eq!(lander_link(&fixture), Some(good));
    assert_eq!(fixture.read("systemctl"), "");

    let held = runtime(&fixture, "held", Some(&replaying(&["old_red"], "failed old red")));
    let same = runtime(&fixture, "same", Some(&replaying(&["old_red"], "failed old red")));
    point(&fixture, "lander", &held);
    point(&fixture, "current", &same);
    let (output, stdout) = promote(&fixture, &[]);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("not a hold, the held runtime replays it the same way: no new failure"), "{stdout}");
    assert_eq!(lander_link(&fixture), Some(same));
}
