use super::functions::{SourceText, measure};
use super::gate::gate;
use super::state::State;
use std::path::PathBuf;
use std::process::Command;

fn complexity_of(source: &str, function: &str) -> usize {
    let files = [SourceText { path: "x.rs".to_owned(), text: source.into() }];
    let measured = measure(&[("h", &files)]).unwrap();
    measured["h/x.rs"].iter().find(|found| found.name == function).unwrap().complexity
}

#[test]
fn nesting_weighs_more_than_the_same_number_of_flat_decisions() {
    assert_eq!(complexity_of("fn flat(a: bool, b: bool) { if a { } if b { } }\n", "flat"), 2);
    assert_eq!(complexity_of("fn nested(a: bool, b: bool) { if a { if b { } } }\n", "nested"), 3);
}

struct Repo {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let state = root.path().join("state");
        let made = Self { _root: root, repo, state };
        made.git(&["init", "-q", "-b", "main"]);
        made
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(&self.repo).output().unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn commit(&self, files: &[(&str, String)]) -> String {
        for (path, text) in files {
            let out = self.repo.join(path);
            std::fs::create_dir_all(out.parent().unwrap()).unwrap();
            std::fs::write(out, text).unwrap();
        }
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", "land"]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn gate(&self, base: &str, head: &str) -> super::gate::GateResult {
        let state = State { root: self.state.clone(), home: self.state.join("home"), graph: self.state.join("graph.db") };
        gate(&state, &self.repo, base, head, None).unwrap()
    }
}

const STRUCTURE: &str = "[thresholds]\nfile_lines = 20\nfunction_lines = 100\nfunction_complexity = 100\n\n[[module]]\nname = \"m\"\npaths = [\"src/m/**\"]\nmay_use = [\"o\"]\n\n[[module]]\nname = \"o\"\npaths = [\"src/o.rs\"]\n";

fn flags_match() -> String {
    "pub fn parse(words: &[&str]) -> Option<(&'static str, bool)> {\n    match words {\n        [\"forget\", name, \"--force\", reason] => Some((reason, true)),\n        [\"forget\", name, \"--to\", successor] => Some((successor, false)),\n        [\"forget\", name] => Some((name, false)),\n        _ => None,\n    }\n}\n".to_owned()
}

#[test]
fn a_split_that_only_moves_complexity_reports_moved_and_file_size_no_longer_refuses() {
    let repo = Repo::new();
    let base = repo.commit(&[
        ("Cargo.toml", "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned()),
        ("structure.toml", STRUCTURE.to_owned()),
        ("src/o.rs", "pub const THING: u8 = 1;\n".to_owned()),
        ("src/m/fleet.rs", flags_match()),
    ]);
    let big: String = (0..30).map(|n| format!("pub const N{n}: u8 = {n};\n")).collect();
    let head = repo.commit(&[
        ("src/m/fleet.rs", "use crate::o::THING;\n\npub fn parse(words: &[&str]) -> Option<(&'static str, bool)> {\n    let _ = THING;\n    flags::parse(words)\n}\n".to_owned()),
        ("src/m/flags.rs", "pub fn parse(words: &[&str]) -> Option<(&'static str, bool)> {\n    match words {\n        [\"forget\", name, \"--force\", reason] => Some((reason, true)),\n        [\"forget\", name, \"--to\", successor] => Some((successor, false)),\n        [\"forget\", name] => Some((name, false)),\n        _ => None,\n    }\n}\n".to_owned()),
        ("src/m/newbig.rs", big),
    ]);
    let result = repo.gate(&base, &head);
    assert!(result.violations.iter().all(|violation| violation.rule != "file-size"), "{:?}", result.violations);
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert!(result.warnings[0].starts_with("file-size src/m/newbig.rs: 30 lines > 20 "), "{:?}", result.warnings);
    let metric = result.metrics.iter().find(|metric| metric.module == "m").unwrap();
    assert_eq!((metric.cognitive, metric.was, metric.status.as_str()), (4, 4, "moved"));
    assert_eq!((metric.fan_in, metric.fan_out), (0, 1));
    churn_is_computed_when_the_state_directory_refuses_the_cache_write();
}

fn churn_is_computed_when_the_state_directory_refuses_the_cache_write() {
    let repo = Repo::new();
    repo.commit(&[("src/a.rs", "pub const A: u8 = 1;\n".to_owned())]);
    std::fs::create_dir_all(repo.state.join("north/gates")).unwrap();
    std::fs::write(repo.state.join("code-health"), "a file where the code-health state directory would be").unwrap();
    let state = State { root: repo.state.clone(), home: repo.state.join("home"), graph: repo.state.join("graph.db") };
    assert!(super::history::churn_at(&state, &repo.repo, "HEAD").is_ok());
    let cached = std::fs::read_dir(state.churn_directory(&repo.repo)).map(|entries| entries.count()).unwrap_or_default();
    assert_eq!(cached, 1, "the churn cache is written under the gates state a confined session may write");
}
