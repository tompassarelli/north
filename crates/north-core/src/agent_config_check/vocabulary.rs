use std::path::{Path, PathBuf};

use crate::platform::git;

/// The names north#1058 retired. The bot login `socrates-agent[bot]` is excluded so code can keep
/// recognising history the old App wrote.
const PATTERNS: [&str; 2] = [r"(^|[^A-Za-z0-9_-])socrates([^A-Za-z0-9_-]|$)", r"(^|[^A-Za-z0-9_-])workforce([^A-Za-z0-9_-]|$)"];

/// Dated records and fixtures keep the names of their day; a new occurrence anywhere else is refused.
const HISTORY: [&str; 4] = [":!docs/rationale/**", ":!harness/lib/fixtures/**", ":!agents/decisions/**", ":!agents/postmortems/**"];

const EXCLUDED: [&str; 3] = [":!*.jsonl", ":!agents/replay/placements/*", ":(exclude,glob)**/agent_config_check/**"];

fn lines(directory: &Path, args: &[&str]) -> Vec<String> {
    git::run(directory, args)
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

pub(super) fn harness_vocabulary(repos: &[PathBuf]) -> (String, bool) {
    let mut report: Vec<String> = Vec::new();
    for repo in repos {
        let Some(top) = lines(repo, &["rev-parse", "--show-toplevel"]).into_iter().next().map(PathBuf::from) else { continue };
        let mut path_args = vec!["ls-files", "--", "*socrates*", "*workforce*"];
        path_args.extend(HISTORY);
        let tracked = lines(&top, &path_args);
        if let Some(first) = tracked.first() {
            report.push(format!("{}: a tracked path names Socrates or workforce (the layer is fleet, and dated records keep the old names): {first}", top.display()));
        }
        let mut args = vec!["grep", "-i", "-l", "-E"];
        for pattern in PATTERNS {
            args.extend(["-e", pattern]);
        }
        args.extend(["--", "."]);
        args.extend(EXCLUDED);
        args.extend(HISTORY);
        let hits = lines(&top, &args);
        if !hits.is_empty() {
            report.push(format!(
                "{}: names Socrates or workforce (the layer is fleet, and dated records keep the old names): {} ",
                top.display(),
                hits.join(" ")
            ));
        }
    }
    if report.is_empty() {
        ("harness vocabulary: no tracked path or text names Socrates or workforce outside dated records".into(), true)
    } else {
        (report.join("\n"), false)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::harness_vocabulary;

    fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git").current_dir(dir.path()).args(args).output().unwrap();
            assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.invalid"]);
        git(&["config", "user.name", "t"]);
        for &(path, text) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        git(&["add", "."]);
        dir
    }

    fn clean(path: &Path) -> bool {
        harness_vocabulary(&[path.to_path_buf()]).1
    }

    #[test]
    fn the_vocabulary_refuses_socrates_and_workforce_outside_dated_records_and_the_old_app_login() {
        let plain = repo(&[("README.md", "North's fleet runs here.\n")]);
        assert!(clean(plain.path()));
        for text in ["Socrates runs here.\n", "The workforce runs here.\n"] {
            let dirty = repo(&[("README.md", text)]);
            let (report, passed) = harness_vocabulary(&[dirty.path().to_path_buf()]);
            assert!(!passed, "{report}");
            assert!(report.contains("README.md"), "{report}");
        }
        let history = repo(&[("docs/rationale/2026-10-12-socrates.md", "Socrates was the workforce.\n")]);
        assert!(clean(history.path()));
        let named = repo(&[("socrates-note.md", "nothing here.\n")]);
        assert!(!clean(named.path()));
        let login = repo(&[("code.rs", "const LOGIN: &str = \"socrates-agent[bot]\";\n")]);
        assert!(clean(login.path()));
    }
}
