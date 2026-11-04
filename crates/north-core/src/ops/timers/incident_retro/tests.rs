use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::super::fleet_session::{Launch, LaunchError, Placement};
use super::system::chunk_number;
use super::{MODEL, Outcome, World, brief, execute, retro, sample};

const TODAY: &str = "2026-11-11";
const STAMP: &str = "202611110900";

fn row(ts: &str, session: &str) -> String {
    format!("{{\"ref\": 1, \"session\": \"{session}\", \"ts\": \"{ts}\", \"text\": \"t\"}}")
}

struct Fake {
    mined: Result<Vec<String>, String>,
    sessions: Vec<String>,
    place: Placement,
    launch: Result<(), LaunchError>,
    saved: RefCell<Vec<(PathBuf, String)>>,
    launched: RefCell<Vec<(String, String, Option<String>)>>,
    discarded: RefCell<Vec<String>>,
    told: RefCell<Vec<String>>,
}

impl Fake {
    fn new(rows: usize) -> Self {
        Self {
            mined: Ok((0..rows).map(|index| row(&format!("2026-11-{:02}T00:00:{:02}Z", 5 + index % 5, index), "s")).collect()),
            sessions: vec!["north-1\tclaude\tgex45\trunning".into()],
            place: Placement::Account("d".into()),
            launch: Ok(()),
            saved: RefCell::default(),
            launched: RefCell::default(),
            discarded: RefCell::default(),
            told: RefCell::default(),
        }
    }
}

impl World for Fake {
    fn mine(&self, _out: &Path) -> Result<Vec<String>, String> {
        self.mined.clone()
    }

    fn save(&self, path: &Path, text: &str) -> Result<(), String> {
        self.saved.borrow_mut().push((path.to_path_buf(), text.to_owned()));
        Ok(())
    }

    fn sessions(&self) -> Result<Vec<String>, String> {
        Ok(self.sessions.clone())
    }

    fn place(&self, _brief: &str) -> Result<Placement, String> {
        Ok(self.place.clone())
    }

    fn worktree(&self, name: &str) -> Result<PathBuf, String> {
        Ok(PathBuf::from("/lanes").join(name))
    }

    fn discard(&self, name: &str, _path: &Path) {
        self.discarded.borrow_mut().push(name.to_owned());
    }

    fn launch(&self, launch: &Launch) -> Result<(), LaunchError> {
        self.launched.borrow_mut().push((launch.name.to_owned(), launch.brief.to_owned(), launch.model.map(str::to_owned)));
        self.launch.clone()
    }

    fn tell_proxy(&self, text: &str) -> Result<(), String> {
        self.told.borrow_mut().push(text.to_owned());
        Ok(())
    }
}

#[test]
fn sample_takes_every_fifth_row_by_timestamp_then_session_from_the_first() {
    let lines = vec![row("2026-11-02", "b"), row("2026-11-01", "z"), row("2026-11-02", "a"), String::new(), row("2026-11-03", "a"), row("2026-11-01", "a"), row("2026-11-04", "a")];
    assert_eq!(sample(&lines, 5), vec![row("2026-11-01", "a"), row("2026-11-04", "a")]);
    assert_eq!(sample(&lines, 2), vec![row("2026-11-01", "a"), row("2026-11-02", "a"), row("2026-11-03", "a")]);
}

#[test]
fn a_run_saves_the_sample_and_launches_one_haiku_session_in_a_south_lane() {
    let fake = Fake::new(12);
    assert_eq!(execute(&fake, Path::new("/state"), TODAY, STAMP, false), 0);
    let saved = fake.saved.borrow();
    assert_eq!(saved[0].0, PathBuf::from("/state/2026-11-11/sample.jsonl"));
    assert_eq!(saved[0].1.lines().count(), 3);
    let launched = fake.launched.borrow();
    assert_eq!(launched.len(), 1);
    assert_eq!(launched[0].0, "incident-retro-202611110900");
    assert_eq!(launched[0].2.as_deref(), Some(MODEL));
    assert!(launched[0].1.contains("(3 of 12 mined candidates)"));
    assert!(fake.told.borrow().is_empty());
}

#[test]
fn a_dry_run_prints_without_listing_placing_or_launching() {
    let fake = Fake { sessions: vec!["incident-retro-202611040900\tclaude\tgex45\trunning".into()], ..Fake::new(7) };
    assert_eq!(retro(&fake, Path::new("/state"), TODAY, STAMP, true), Outcome::Launched("would launch on 2 of 7 candidates in /state/2026-11-11/sample.jsonl".into()));
    assert!(fake.launched.borrow().is_empty());
}

#[test]
fn a_live_retro_session_skips_the_week() {
    let fake = Fake { sessions: vec!["incident-retro-202611040900\tclaude\tgex45\trunning".into()], ..Fake::new(7) };
    assert!(matches!(retro(&fake, Path::new("/state"), TODAY, STAMP, false), Outcome::Skipped(_)));
    let exited = Fake { sessions: vec!["incident-retro-202611040900\tclaude\tgex45\texited 0".into()], ..Fake::new(7) };
    assert!(matches!(retro(&exited, Path::new("/state"), TODAY, STAMP, false), Outcome::Launched(_)));
}

#[test]
fn no_candidates_launches_nothing_and_exits_zero() {
    let fake = Fake::new(0);
    assert_eq!(execute(&fake, Path::new("/state"), TODAY, STAMP, false), 0);
    assert!(fake.saved.borrow().is_empty() && fake.launched.borrow().is_empty() && fake.told.borrow().is_empty());
}

#[test]
fn a_held_placement_tells_the_proxy_and_exits_zero_and_a_failed_mine_exits_one() {
    let held = Fake { place: Placement::Held("held: no Claude account is available".into()), ..Fake::new(5) };
    assert_eq!(execute(&held, Path::new("/state"), TODAY, STAMP, false), 0);
    assert!(held.told.borrow()[0].contains("held, next week's run retries"));
    let failed = Fake { mined: Err("north incident-mine exited 1: convo failed".into()), ..Fake::new(5) };
    assert_eq!(execute(&failed, Path::new("/state"), TODAY, STAMP, false), 1);
    assert!(failed.told.borrow()[0].contains("failed: north incident-mine exited 1"));
}

#[test]
fn a_failed_launch_discards_its_lane() {
    let fake = Fake { launch: Err(LaunchError::Failed("north fleet launch exited 2: brief refused".into())), ..Fake::new(5) };
    assert_eq!(execute(&fake, Path::new("/state"), TODAY, STAMP, false), 1);
    assert_eq!(fake.discarded.borrow().as_slice(), ["incident-retro-202611110900"]);
}

#[test]
fn the_brief_names_its_routing_lines_and_the_sample_path() {
    let text = brief(TODAY, Path::new("/state/2026-11-11/sample.jsonl"), 3, 12);
    for line in ["Category: mechanical", "Tier: worker-haiku", "Spec: measured", "Verify: none", "Files: south agents/incidents.jsonl, south agents/rules.toml"] {
        assert!(text.lines().any(|row| row == line), "{line}");
    }
    assert!(text.contains("`/state/2026-11-11/sample.jsonl`"));
}

#[test]
fn chunk_files_sort_by_number() {
    assert_eq!(chunk_number(Path::new("/x/chunk-10.jsonl")), Some(10));
    assert_eq!(chunk_number(Path::new("/x/sample.jsonl")), None);
}
