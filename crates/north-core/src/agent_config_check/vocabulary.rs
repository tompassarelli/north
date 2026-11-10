use std::path::{Path, PathBuf};

use crate::platform::git;

/// The two agent names north#1058 retired, spelled with escapes so this file never contains them. The old
/// App's bot login is excluded so code can keep recognising history it wrote.
const RETIRED: [&str; 2] = ["socr\u{61}tes", "work\u{66}orce"];

/// Dated records and fixtures keep the names of their day; a new occurrence anywhere else is refused.
const HISTORY: [&str; 5] = [":!docs/rationale/**", ":!harness/lib/fixtures/**", ":!agents/decisions/**", ":!agents/postmortems/**", ":!agents/replay/**"];

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
        let globs = RETIRED.map(|name| format!("*{name}*"));
        let mut path_args = vec!["ls-files", "--", &globs[0], &globs[1]];
        path_args.extend(HISTORY);
        let tracked = lines(&top, &path_args);
        if let Some(first) = tracked.first() {
            report.push(format!("{}: a tracked path names a retired agent name (the layer is fleet, and dated records keep the old names): {first}", top.display()));
        }
        let patterns = RETIRED.map(|name| format!("(^|[^A-Za-z0-9_-]){name}([^A-Za-z0-9_-]|$)"));
        let mut args = vec!["grep", "-i", "-l", "-E"];
        for pattern in &patterns {
            args.extend(["-e", pattern.as_str()]);
        }
        args.extend(["--", "."]);
        args.extend(EXCLUDED);
        args.extend(HISTORY);
        let hits = lines(&top, &args);
        if !hits.is_empty() {
            report.push(format!(
                "{}: names a retired agent name (the layer is fleet, and dated records keep the old names): {} ",
                top.display(),
                hits.join(" ")
            ));
        }
    }
    if report.is_empty() {
        ("harness vocabulary: no tracked path or text names a retired agent name outside dated records".into(), true)
    } else {
        (report.join("\n"), false)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::{harness_vocabulary, RETIRED};

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
    fn the_vocabulary_refuses_the_retired_names_outside_dated_records_and_the_old_app_login() {
        let plain = repo(&[("README.md", "North's fleet runs here.\n")]);
        assert!(clean(plain.path()));
        for name in RETIRED {
            let text = format!("The {name} runs here.\n");
            let dirty = repo(&[("README.md", text.as_str())]);
            let (report, passed) = harness_vocabulary(&[dirty.path().to_path_buf()]);
            assert!(!passed, "{report}");
            assert!(report.contains("README.md"), "{report}");
        }
        let [first, second] = RETIRED;
        let history_text = format!("{first} was the {second}.\n");
        let history_path = format!("docs/rationale/2026-10-12-{first}.md");
        let history = repo(&[(history_path.as_str(), history_text.as_str())]);
        assert!(clean(history.path()));
        let replay_text = format!("{first} is Tom's agent organization.\n");
        let replay = repo(&[("agents/replay/policy-pre-cut.md", replay_text.as_str())]);
        assert!(clean(replay.path()));
        let named_path = format!("{first}-note.md");
        let named = repo(&[(named_path.as_str(), "nothing here.\n")]);
        assert!(!clean(named.path()));
        let login_text = format!("const LOGIN: &str = \"{first}-agent[bot]\";\n");
        let login = repo(&[("code.rs", login_text.as_str())]);
        assert!(clean(login.path()));
    }
}
