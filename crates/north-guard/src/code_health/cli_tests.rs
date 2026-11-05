use super::cli::{Io, run_with as run};
use super::config::{Thresholds, parse};
use super::state::State;
use std::path::PathBuf;
use std::process::Command;

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

    fn write(&self, files: &[(&str, String)]) {
        for (path, text) in files {
            let out = self.repo.join(path);
            std::fs::create_dir_all(out.parent().unwrap()).unwrap();
            std::fs::write(out, text).unwrap();
        }
    }

    fn commit(&self, files: &[(&str, String)]) -> String {
        self.write(files);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", "land"]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn north(&self, args: &[&str]) -> (u8, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let arguments: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let state = State { root: self.state.clone(), home: self.state.join("home") };
        let code = run(&arguments, &mut Io { out: &mut out, err: &mut err, cwd: self.repo.clone(), state });
        (code, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned())
    }

    fn fire(&self, extra: &[&str]) -> (u8, String) {
        let (code, out, err) = self.north(&[&["signal"], extra].concat());
        assert_eq!(err, "");
        (code, out.trim().to_owned())
    }
}

fn function(name: &str, n: usize) -> String {
    let body: Vec<String> = (0..n).map(|i| format!("  const v{i} = {i};")).collect();
    format!("export function {name}() {{\n{}\n}}\n", body.join("\n"))
}

fn filler(n: usize) -> String {
    (0..n).map(|i| format!("export const c{i} = {i};\n")).collect()
}

const STRUCTURE: &str = "[thresholds]\nfile_lines = 200\nfunction_lines = 80\nfunction_complexity = 15\nhotspot_top = 1\n[[module]]\nname = \"a\"\npaths = [\"src/a/**\"]\nmay_use = [\"b\"]\n[[module]]\nname = \"b\"\npaths = [\"src/b/**\"]\n[[module]]\nname = \"c\"\npaths = [\"src/c/**\"]\n";

fn base_files() -> Vec<(&'static str, String)> {
    vec![
        ("structure.toml", STRUCTURE.to_owned()),
        ("src/a/main.ts", "import { y } from \"../b/y\";\nexport const m = y;\n".to_owned()),
        ("src/a/legacy.ts", function("legacy", 100)),
        ("src/a/grow.ts", filler(190)),
        ("src/b/x.ts", "import { y } from \"./y\";\nexport const x = y;\n".to_owned()),
        ("src/b/y.ts", "export const y = 1;\n".to_owned()),
        ("src/c/z.ts", "export const z = 1;\n".to_owned()),
    ]
}

fn debt() -> Vec<(&'static str, String)> {
    vec![
        ("src/b/y.ts", "import { x } from \"./x\";\nexport const y = 1;\nexport const yx = () => x;\n".to_owned()),
        ("src/b/x.ts", "import { y } from \"./y\";\nimport { z } from \"../c/z\";\nexport const x = y + z;\n".to_owned()),
        ("src/a/big.ts", function("big", 100)),
        ("src/a/grow.ts", filler(230)),
    ]
}

struct Fixture {
    repo: Repo,
    base: String,
    debt: String,
    touched: String,
}

fn fixture() -> Fixture {
    let repo = Repo::new();
    let base = repo.commit(&base_files());
    let debt = repo.commit(&debt());
    repo.git(&["checkout", "-q", "-b", "legacy", &base]);
    let touched = repo.commit(&[("src/a/legacy.ts", function("legacy", 110))]);
    repo.git(&["checkout", "-q", &base]);
    Fixture { repo, base, debt, touched }
}

fn rules_of(out: &str) -> Vec<String> {
    let mut rules: Vec<String> = out.lines().filter(|line| !line.is_empty()).map(|line| line.split(' ').take(2).collect::<Vec<_>>().join(" ")).collect();
    rules.sort();
    rules
}

const EXPECTED: [&str; 4] = ["cycle src/b/y.ts", "file-size src/a/grow.ts", "function src/a/big.ts:1", "layer src/b/x.ts"];

fn restore(repo: &Repo) {
    repo.git(&["checkout", "-q", "--", "."]);
    let _ = std::fs::remove_file(repo.repo.join("src/a/big.ts"));
}

#[test]
fn gate_names_each_new_debt_once_and_passes_untouched_legacy_debt() {
    let f = fixture();
    let (code, out, err) = f.repo.north(&["gate", &f.base, &f.debt]);
    assert_eq!((code, err.as_str()), (1, ""), "{out}");
    assert_eq!(rules_of(&out), EXPECTED);
    assert_eq!(f.repo.north(&["gate", &f.base, &f.touched]), (0, String::new(), String::new()));
}

#[test]
fn check_names_the_same_debt_for_dirty_worktree_files() {
    let f = fixture();
    f.repo.write(&debt());
    let mut lines = String::new();
    for (path, _) in debt() {
        let (_, out, err) = f.repo.north(&["check", path]);
        assert_eq!(err, "");
        lines.push_str(&out);
    }
    assert_eq!(rules_of(&lines), EXPECTED);
    assert_eq!(f.repo.north(&["check", "src/a/main.ts"]).1, "");
    restore(&f.repo);
}

#[test]
fn module_limits_override_only_named_defaults_in_gate_and_edit_feedback() {
    let f = fixture();
    let text = STRUCTURE.replace("name = \"a\"", "name = \"a\"\nthresholds = { file_lines = 300, function_lines = 120 }");
    let config = parse(Some(&text), Default::default()).unwrap();
    assert_eq!(config.thresholds_of("src/a/big.ts"), Thresholds { file_lines: 300.0, function_lines: 120.0, function_complexity: 15.0, hotspot_top: 1.0 });
    assert_eq!(config.thresholds_of("src/b/x.ts"), config.thresholds);
    assert_eq!(config.thresholds_of("unknown.ts"), config.thresholds);
    let map = f.repo.state.join("override.toml");
    std::fs::create_dir_all(map.parent().unwrap()).unwrap();
    std::fs::write(&map, &text).unwrap();
    let map = map.to_str().unwrap();
    let (code, out, err) = f.repo.north(&["gate", "--config", map, &f.base, &f.debt]);
    assert_eq!((code, err.as_str()), (1, ""));
    assert_eq!(rules_of(&out), ["cycle src/b/y.ts", "layer src/b/x.ts"]);
    f.repo.write(&debt());
    for path in ["src/a/big.ts", "src/a/grow.ts"] {
        assert_eq!(f.repo.north(&["check", "--config", map, path]), (0, String::new(), String::new()));
    }
    restore(&f.repo);
}

const MAP: &str = "[thresholds]\nfile_lines = 500\n";

fn scratch(first: &[(&'static str, String)], review: bool) -> Repo {
    let repo = Repo::new();
    repo.commit(&[&[("a.ts", "export const a = 1;\n".to_owned())], first].concat());
    if review {
        assert_eq!(repo.north(&["review-done"]).0, 0);
    }
    repo
}

#[test]
fn a_repo_without_structure_toml_is_not_enrolled_and_stays_quiet() {
    let repo = scratch(&[], false);
    assert_eq!(repo.fire(&[]), (1, "quiet".to_owned()));
    repo.commit(&[("structure.toml", MAP.to_owned())]);
    let (code, out) = repo.fire(&[]);
    assert!(out.starts_with("FIRE no review recorded"), "{out}");
    assert_eq!(code, 0);
}

#[test]
fn signal_judges_the_named_revision_not_a_stale_checkout() {
    let repo = scratch(&[], false);
    repo.git(&["checkout", "-qb", "ahead"]);
    repo.commit(&[("structure.toml", MAP.to_owned())]);
    repo.git(&["checkout", "-q", "-"]);
    assert_eq!(repo.fire(&[]).1, "quiet");
    let (code, out) = repo.fire(&["--rev", "ahead"]);
    assert!(out.starts_with("FIRE no review recorded"), "{out}");
    assert_eq!(code, 0);
}

#[test]
fn the_commit_count_fires_at_its_limit_days_before_the_seven_day_signal_could() {
    let repo = scratch(&[("structure.toml", "[review]\ncommits = 3\nhand_lines_added = 0\ntest_lines_changed = 0\ndays = 7\n".to_owned())], true);
    repo.commit(&[("b.ts", "export const b = 1;\n".to_owned())]);
    repo.commit(&[("c.ts", "export const c = 1;\n".to_owned())]);
    assert_eq!(repo.fire(&[]).1, "quiet");
    repo.commit(&[("d.ts", "export const d = 1;\n".to_owned())]);
    let fired = repo.fire(&[]).1;
    assert!(fired.starts_with("FIRE 3 commits since review"), "{fired}");
    assert!(!fired.contains("days since review"), "{fired}");
}

#[test]
fn test_file_churn_counts_lines_added_plus_removed_under_test_paths_and_fires_at_its_limit() {
    let repo = scratch(&[("structure.toml", "[review]\ncommits = 0\nhand_lines_added = 0\ntest_lines_changed = 10\ndays = 0\n".to_owned()), ("tests/old.test.ts", "o1\no2\no3\no4\no5\n".to_owned())], true);
    repo.commit(&[("tests/old.test.ts", "n1\nn2\nn3\n".to_owned()), ("src.ts", "x\n".repeat(50))]);
    assert_eq!(repo.fire(&[]).1, "quiet");
    repo.commit(&[("tests/new.test.ts", "1\n2\n3\n4\n".to_owned())]);
    assert_eq!(repo.fire(&[]).1, "FIRE 12 test lines changed since review (limit 10)");
}

#[test]
fn a_moved_file_counts_only_the_lines_its_move_changed_so_a_crate_split_does_not_read_as_new_code() {
    let source = "export const x = 1;\n".repeat(40);
    let repo = scratch(&[("structure.toml", "[review]\ncommits = 0\nhand_lines_added = 5\ntest_lines_changed = 5\ndays = 0\n".to_owned()), ("src/big.ts", source.clone()), ("tests/big.test.ts", "t\n".repeat(40))], true);
    repo.git(&["mv", "src", "crate"]);
    repo.git(&["mv", "tests", "crate-tests"]);
    repo.commit(&[("crate/big.ts", format!("{source}export const y = 2;\n"))]);
    assert_eq!(repo.fire(&[]).1, "quiet");
    repo.commit(&[("crate/new.ts", "export const n = 1;\n".repeat(5))]);
    assert_eq!(repo.fire(&[]).1, "FIRE 6 hand lines added since review (limit 5)");
}
