use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

const RECORD: &str = "ts/scripts/wisp/cue-models.tsv";
const PRIMARY_INPUTS: usize = 40;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

struct Repo {
    _root: tempfile::TempDir,
    path: std::path::PathBuf,
    base: String,
}

struct Scan {
    status: i32,
    output: String,
}

impl Repo {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("repo");
        fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-q", "-b", "main"]);
        Self {
            _root: root,
            path,
            base: String::new(),
        }
    }

    fn commit(&self, message: &str) -> String {
        git(&self.path, &["add", "-A", "."]);
        git(
            &self.path,
            &["commit", "-q", "--allow-empty", "-m", message],
        );
        git(&self.path, &["rev-parse", "HEAD"])
    }

    fn mark_base(&mut self, message: &str) {
        self.base = self.commit(message);
    }

    fn reset(&self) {
        git(&self.path, &["reset", "-q", "--hard", &self.base]);
        git(&self.path, &["clean", "-qfd"]);
    }

    fn config(&self, arguments: &[&str]) {
        let mut all = vec!["config", "--file", ".safe-push"];
        all.extend(arguments);
        git(&self.path, &all);
    }

    fn scan(&self, name: &str) -> Scan {
        let head = git(&self.path, &["rev-parse", "HEAD"]);
        let output = Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(&self.path)
            .args(["land", "scan", name, &format!("{}..{head}", self.base)])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Scan {
            status: output.status.code().unwrap_or(-1),
            output: format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }
}

fn sweep_row(sha: &str, first: [u32; 2], second: [u32; 2]) -> String {
    format!(
        "{{\"variant\":\"base\",\"policy_sha256\":\"{sha}\",\"scenarios\":{{\"answer-first-line\":{first:?},\"cleanup\":{second:?}}}}}\n"
    )
}

#[test]
fn policy_sweep() {
    let mut repo = Repo::new();
    let policy = repo.path.join("agents/AGENTS.md");
    write(&policy, "Policy\n\n- Keep it short.\n");
    write(
        &repo.path.join("agents/replay/answer-first-line.json"),
        "{\"id\":\"answer-first-line\"}\n",
    );
    write(&repo.path.join("agents/replay/cleanup.json"), "{}\n");
    write(&repo.path.join("NOTES.md"), "notes\n");
    repo.mark_base("operator policy");
    let sweep = repo.path.join("agents/replay/sweep-2026-10-28.jsonl");
    for case in [
        "no-row",
        "row",
        "row-one-trial",
        "row-other-sha",
        "row-in-worktree-only",
        "no-policy-change",
    ] {
        repo.reset();
        if case == "no-policy-change" {
            write(&repo.path.join("NOTES.md"), "notes\nmore notes\n");
            repo.commit(case);
        } else {
            write(&policy, "Policy\n\n- Keep it short.\n- Ask once.\n");
            let sha = format!("{:x}", Sha256::digest(fs::read(&policy).unwrap()));
            match case {
                "row" => write(&sweep, &sweep_row(&sha, [3, 3], [2, 3])),
                "row-one-trial" => write(&sweep, &sweep_row(&sha, [1, 1], [1, 1])),
                "row-other-sha" => write(&sweep, &sweep_row(&"0".repeat(64), [3, 3], [3, 3])),
                _ => {}
            }
            repo.commit(case);
            if case == "row-in-worktree-only" {
                write(&sweep, &sweep_row(&sha, [3, 3], [3, 3]));
            }
        }
        let scan = repo.scan("policy-sweep");
        if matches!(case, "row" | "no-policy-change") {
            assert_eq!(scan.status, 0, "{case}: {}", scan.output);
        } else {
            assert_eq!(scan.status, 1, "{case}: {}", scan.output);
            assert!(
                scan.output.contains("makes agents/AGENTS.md"),
                "{case}: {}",
                scan.output
            );
        }
    }
}

fn inputs() -> Vec<String> {
    (0..PRIMARY_INPUTS)
        .map(|index| format!("inputs/primary-{index:02}.ts"))
        .collect()
}

fn rewrite(path: &Path, count: usize) {
    let text = fs::read_to_string(path).unwrap();
    let rows: Vec<String> = text
        .lines()
        .enumerate()
        .map(|(index, row)| {
            if index < count {
                row.replace("original-provenance", "changed-provenance")
            } else {
                row.to_owned()
            }
        })
        .collect();
    fs::write(path, rows.join("\n") + "\n").unwrap();
}

#[test]
fn generated_rewrite() {
    let mut repo = Repo::new();
    let inputs = inputs();
    for input in &inputs {
        write(&repo.path.join(input), "original primary input\n");
        repo.config(&["--add", &format!("generated.{RECORD}.input"), input]);
    }
    let record: String = (0..1000)
        .map(|row| format!("classic-use\t{row}\tvisible\toriginal-provenance\n"))
        .collect();
    write(&repo.path.join(RECORD), &record);
    write(&repo.path.join("authored.tsv"), &record);
    repo.mark_base("recorded generated primary inventory");
    let key = format!("generated.{RECORD}.input");
    let section = format!("generated.{RECORD}");
    for journey in [
        "churn",
        "proportional",
        "five-percent",
        "full-input",
        "missing",
        "removed-authority",
        "untracked-input",
        "authored",
        "added-input",
        "removed-input",
        "unchanged-missing",
        "unchanged-removed",
        "absent-output",
    ] {
        repo.reset();
        let (mut count, mut changed, mut output) = (1000, 0, RECORD);
        match journey {
            "churn" => changed = 1,
            "proportional" => count = 500,
            "five-percent" => changed = 2,
            "full-input" => changed = PRIMARY_INPUTS,
            "missing" => repo.config(&["--replace-all", &key, ""]),
            "removed-authority" => repo.config(&["--remove-section", &section]),
            "untracked-input" => repo.config(&["--add", &key, "absent.ts"]),
            "authored" => output = "authored.tsv",
            "added-input" => {
                write(&repo.path.join("new-primary.ts"), "new primary\n");
                repo.config(&["--add", &key, "new-primary.ts"]);
            }
            "removed-input" => git(&repo.path, &["rm", "-q", &inputs[0]]).truncate(0),
            "unchanged-missing" => {
                count = 0;
                repo.config(&["--replace-all", &key, ""]);
            }
            "unchanged-removed" => {
                count = 0;
                repo.config(&["--remove-section", &section]);
            }
            _ => {
                count = 0;
                repo.config(&["--add", "generated.absent.tsv.input", &inputs[0]]);
            }
        }
        for input in &inputs[..changed] {
            write(&repo.path.join(input), "original primary input\n\n");
        }
        rewrite(&repo.path.join(output), count);
        repo.commit(journey);
        let scan = repo.scan("generated-rewrite");
        if matches!(
            journey,
            "proportional" | "five-percent" | "full-input" | "authored"
        ) {
            assert_eq!(scan.status, 0, "{journey}: {}", scan.output);
        } else {
            assert_eq!(scan.status, 1, "{journey}: {}", scan.output);
            assert!(
                scan.output.ends_with("NOT pushed.\n"),
                "{journey}: {}",
                scan.output
            );
            let named = if journey == "absent-output" {
                "absent.tsv"
            } else {
                RECORD
            };
            assert!(scan.output.contains(named), "{journey}: {}", scan.output);
        }
    }
}

#[test]
fn generated_rewrite_refuses_a_bare_generated_input() {
    let mut repo = Repo::new();
    write(&repo.path.join("x"), "primary\n");
    repo.mark_base("primary input");
    repo.config(&["generated.input", "x"]);
    repo.commit("bare generated input");
    let scan = repo.scan("generated-rewrite");
    assert_eq!(scan.status, 1, "{}", scan.output);
    assert_eq!(
        scan.output,
        "safe-push: ✗ : missing or invalid generated input authority; declare repeated input = <tracked primary file> in [generated \"\"]; NOT pushed.\n"
    );
}
