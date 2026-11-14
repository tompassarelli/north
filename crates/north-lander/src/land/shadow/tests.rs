use std::fs;
use std::path::Path;
use std::process::Command;

use north_base::foundation::shim::write_executable;

use super::super::slots::verdict_key;
use super::{Settings, run};

const STORE: &str = r#"#!/usr/bin/env bash
dir="STORE_DIR"
[ "$1 $2 $3" = "capacity offload verdict" ] || exit 2
shift 3
if [ "$1" = hit ]; then
  shift
  for key in "$@"; do [ -f "$dir/$key" ] && echo "$key $(cat "$dir/$key")"; done
  exit 0
fi
[ "$1 $2 $4" = "record --host --key" ] || exit 2
host="$3"; key="$5"
if [ "$6" = --pass ]; then echo "$host 1 $7" >"$dir/$key"; else rm -f "$dir/$key"; fi
"#;

fn git(repo: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(repo).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn script(path: &Path, text: &str) -> String {
    write_executable(path, text);
    path.display().to_string()
}

#[test]
fn a_verdict_the_shadow_tick_records_is_found_by_the_slot_runners_lookup_for_the_same_lane_on_the_same_main() {
    let world = tempfile::tempdir().unwrap();
    let repo = world.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@example.invalid"]);
    git(&repo, &["config", "user.name", "t"]);
    fs::write(repo.join(".safe-push"), "[land]\n\tcheck = src/ :: cargo nextest run $SAFE_PUSH_LAND_PACKAGES\n").unwrap();
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/main.rs"), "fn main() {}\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "main"]);
    let main = git(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("src/lane.rs"), "fn lane() {}\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "lane"]);
    let merged = git(&repo, &["rev-parse", "HEAD"]);

    let store = world.path().join("store");
    fs::create_dir_all(&store).unwrap();
    let north = script(&world.path().join("north"), &STORE.replace("STORE_DIR", &store.display().to_string()));
    let checker = script(&world.path().join("checker"), "#!/usr/bin/env bash\nexit 0\n");
    let settings = Settings { repo: repo.clone(), state: world.path().join("state"), north: vec![north.clone()], queue: world.path().join("queue"), host: "lander".to_owned(), checker: vec![checker], chooser: Vec::new() };

    let command = "cargo nextest run $SAFE_PUSH_LAND_PACKAGES".to_owned();
    let environment = verdict_key::lane_environment(&repo, &main, &merged, None, std::slice::from_ref(&command));
    let key = verdict_key::key(&repo, &main, &merged, &command, &environment).expect("a key");
    let found = run::Found { head: merged.clone(), lines: vec![(command.clone(), key)] };

    let outcome = run::check(&settings, &repo, &main, "gex45", &found, &world.path().join("logs"));
    assert_eq!(outcome, (1, 0));

    let slot_runner_key = verdict_key::key(&repo, &main, &merged, &command, &environment).expect("a key");
    let asked = Command::new(&north).args(verdict_key::hit_arguments(&[slot_runner_key.as_str()])).output().unwrap();
    let hits = verdict_key::hits(&String::from_utf8_lossy(&asked.stdout));
    let (host, pass) = hits.get(&slot_runner_key).expect("the slot runner finds the shadow's pass");
    assert_eq!((host.as_str(), pass.sha.as_str()), ("gex45", merged.as_str()));
}

#[test]
fn a_shadow_round_skips_the_queued_candidate_the_lander_is_checking() {
    use north_base::land_ticket::{Phase, Records};
    use super::super::queue::Ticket;
    use super::pick::not_checking;

    let queue = tempfile::tempdir().unwrap();
    let held = queue.path().join("held.json");
    let waiting = queue.path().join("waiting.json");
    let mut records = Records::open(queue.path()).unwrap();
    records.moved(&held, Phase::Queued, "queued").unwrap();
    records.moved(&waiting, Phase::Queued, "queued").unwrap();
    records.checking(std::slice::from_ref(&held)).unwrap();

    let queued = vec![(held, Ticket::default()), (waiting.clone(), Ticket::default())];
    let kept: Vec<_> = not_checking(queued, queue.path()).into_iter().map(|(path, _)| path).collect();
    assert_eq!(kept, vec![waiting]);
}
