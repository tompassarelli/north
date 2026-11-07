use super::cli::{Io, run_with as run};
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

const TWO_MODULES: &str = "[review]\ncommits = 0\nhand_lines_added = 0\ntest_lines_changed = 0\ndays = 0\nhidden_links = 1\n\n[[module]]\nname = \"left\"\npaths = [\"src/left/**\"]\n\n[[module]]\nname = \"right\"\npaths = [\"src/right/**\"]\n";

#[test]
fn changes_since_review_fire_on_a_hidden_link_show_in_the_budget_and_record_files_per_commit_and_the_reverted_share() {
    let repo = scratch(&[("structure.toml", TWO_MODULES.to_owned()), ("src/left/a.ts", "let _ = 0;\n".to_owned())], true);
    let mut last = String::new();
    for n in 1..4 {
        last = repo.commit(&[("src/left/a.ts", format!("let _ = {n};\n")), ("src/right/b.ts", format!("export const b = {n};\n"))]);
    }
    repo.git(&["revert", "--no-edit", &last]);
    repo.commit(&[("src/left/c.ts", "export const c = 1;\n".to_owned())]);
    assert_eq!(repo.fire(&["--record"]).1, "FIRE hidden links grew 0 -> 1 since review (limit +1)");
    let (code, out, err) = repo.north(&["budget"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("Hidden links: 1 ") && out.contains("     4  left <-> right  e.g. src/left/a.ts <-> src/right/b.ts"), "{out}");
    assert!(out.contains("Special cases: 1 ") && out.contains("  left     1    0    0"), "{out}");
    let name = repo.repo.file_name().unwrap().to_string_lossy().into_owned();
    let score = std::fs::read_to_string(repo.state.join("code-health").join(name).join("loop.txt")).unwrap();
    assert!(score.starts_with("median 2 hand files per feature commit over 4 feature commits since review "), "{score}");
    assert!(score.trim_end().ends_with(", 1 reworked (25.0%)"), "{score}");
}
