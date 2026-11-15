use std::path::{Path, PathBuf};

use north_base::platform::git;

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
