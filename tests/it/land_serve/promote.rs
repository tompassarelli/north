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

#[test]
fn promote_replays_each_recorded_round_then_shadows_the_queue_before_it_swaps_the_lander_link() {
    let mut fixture = served();
    let pending = fixture.lane("e", "e.txt", "e\n");
    let ticket = fixture.enqueue(&pending, None);
    let main = fixture.main();
    let good = runtime(&fixture, "good", None);
    point(&fixture, "current", &good);
    let (output, stdout) = promote(&fixture, &["--dry-run"]);
    assert!(output.status.success());
    let lines: Vec<&str> = stdout.lines().collect();
    let landed = lines.iter().position(|line| line.contains("round lane-a,lane-b,lane-c on ") && line.ends_with("recorded landed replayed landed: matches")).expect("the landed round is replayed");
    let conflict = lines.iter().position(|line| line.contains("round lane-d on ") && line.ends_with("recorded conflict replayed conflict: matches")).expect("the conflict round is replayed");
    let shadow = lines.iter().position(|line| line.ends_with("shadow: 1 pass(es), 1 candidate(s) checked, 0 error(s), none repeated")).expect("the shadow result follows the replay");
    assert!(landed.max(conflict) < shadow);
    assert!(stdout.contains("shadow pass 1: 1 queued lane(s) of"));
    assert!(lines.last().unwrap().contains("dry run: 2 round(s) matched;"));
    assert_eq!(lander_link(&fixture), None, "a dry run leaves the lander link alone");
    assert_eq!(fixture.read("systemctl"), "");
    assert_eq!(fixture.main(), main, "neither the replay nor the shadow pushes");
    assert!(ticket.exists(), "the shadow decides nothing in the live queue");
    let (output, stdout) = promote(&fixture, &[]);
    assert!(output.status.success());
    assert_eq!(lander_link(&fixture), Some(good.clone()));
    assert_eq!(fixture.read("systemctl"), "--user restart north-land*\n");
    assert!(stdout.lines().last().unwrap().ends_with("restarted north-land*"));
    let (output, stdout) = promote(&fixture, &[]);
    assert!(output.status.success());
    assert!(stdout.contains(&format!("the lander already runs {}", good.display())));
}

#[test]
fn promote_refuses_a_candidate_whose_replayed_verdict_differs_and_keeps_the_lander_link() {
    let fixture = served();
    let good = runtime(&fixture, "good", None);
    point(&fixture, "lander", &good);
    let drifted = runtime(&fixture, "drifted", Some(&format!("#!/usr/bin/env bash\nif [ \"$1 $2\" = \"land replay\" ]; then echo 'replay: verdict failed drifted judgement'; exit 0; fi\nexec {} \"$@\"\n", env!("CARGO_BIN_EXE_north"))));
    point(&fixture, "current", &drifted);
    let (output, stdout) = promote(&fixture, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout.contains("refused: round lane-") && stdout.contains("but the candidate replayed failed drifted judgement; the lander link is unchanged"), "{stdout}");
    assert!(!stdout.contains("shadow:"), "a replay mismatch refuses before the shadow");
    assert_eq!(lander_link(&fixture), Some(good));
    assert_eq!(fixture.read("systemctl"), "");
}

#[test]
fn promote_replays_a_refused_round_only_on_the_main_its_decision_recorded() {
    let fixture = served();
    let done = fixture.queue().join("done");
    let refused = fs::read_dir(&done).unwrap().filter_map(Result::ok).map(|entry| entry.path()).find(|path| path.to_string_lossy().ends_with("lane-d.json")).expect("lane d was decided");
    let ticket = north_landing::land::queue::read(&refused).unwrap();
    assert!(ticket.decided_on.as_deref().is_some_and(|main| main.len() == 40), "the lander records the main it decided lane d on: {:?}", ticket.decided_on);
    north_landing::land::queue::write(&refused, &north_landing::land::queue::Ticket { decided_on: None, ..ticket }).unwrap();
    let good = runtime(&fixture, "good", None);
    point(&fixture, "current", &good);
    let (output, stdout) = promote(&fixture, &["--dry-run"]);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("round lane-d recorded conflict: not replayable, the lander did not record the main it was decided on"), "a refusal with no recorded main is not replayed on a guessed one: {stdout}");
    assert!(stdout.lines().any(|line| line.contains("round lane-a,lane-b,lane-c on ") && line.ends_with("recorded landed replayed landed: matches")), "{stdout}");
}
