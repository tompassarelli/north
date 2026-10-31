use std::path::{Path, PathBuf};

use crate::platform::git;

const PATTERNS: [&str; 2] = [r"(^|[^A-Za-z0-9_-])socrates/", "Socrates is the agent machinery"];

const EXCLUDED: [&str; 4] = [":!*.jsonl", ":!agents/incident-baseline.toml", ":!agents/replay/placements/*", ":!src/agent_config_check/*"];

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
        let tracked = lines(&top, &["ls-files", "--", "socrates/*"]);
        if let Some(first) = tracked.first() {
            report.push(format!("{}: the harness lives under harness/, not socrates/: {first}", top.display()));
        }
        let mut args = vec!["grep", "-l", "-E"];
        for pattern in PATTERNS {
            args.extend(["-e", pattern]);
        }
        args.extend(["--", "."]);
        args.extend(EXCLUDED);
        let hits = lines(&top, &args);
        if !hits.is_empty() {
            report.push(format!(
                "{}: names a socrates/ path or calls Socrates the agent machinery (Socrates is the workforce, North the harness): {} ",
                top.display(),
                hits.join(" ")
            ));
        }
    }
    if report.is_empty() {
        ("harness vocabulary: no socrates/ path and no \"Socrates is the agent machinery\"".into(), true)
    } else {
        (report.join("\n"), false)
    }
}
