use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use north_base::platform::replay_verdict::{self, PROOF_SUFFIX, Verdict};
use north_landing_gates::issue_box_lint;

use crate::platform::process;
use crate::{Range, gate_shadow, git, test_file};

const SELF: &str = "detector-proof";
const REGISTRY: &str = "harness/gates.toml";
const DETECTOR_MARK: &str = "(detector ";
const CASES: &str = "harness/cases/";
const SHORT_SHA: usize = 12;
const GH_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

fn ticked_boxes(message: &str) -> Vec<(String, String)> {
    let mut item: Option<String> = None;
    let mut found = Vec::new();
    for line in message.lines() {
        if let Some(value) = line.strip_prefix("Item:") {
            item = value.split_whitespace().next().and_then(|item| issue_box_lint::issue(item).ok()).map(|(repo, number)| format!("{}#{number}", issue_box_lint::slug(repo)));
        } else if let (Some(value), Some(item)) = (line.strip_prefix("Box:"), &item) {
            let text = value.trim();
            if !text.is_empty() {
                found.push((item.clone(), text.to_owned()));
            }
        }
    }
    found
}

fn open_box<'a>(body: &'a str, wanted: &str) -> Option<&'a str> {
    let open: Vec<&str> = body.lines().filter_map(|line| line.trim_start().strip_prefix("- [ ]").or_else(|| line.trim_start().strip_prefix("* [ ]"))).filter(|text| text.contains(wanted)).collect();
    match open.as_slice() {
        [text] => Some(text),
        _ => None,
    }
}

fn commits(directory: &Path, fork: &str, head: &str) -> Vec<(String, String)> {
    git::text(directory, &["log", "--reverse", "--format=%H%x1f%B%x1e", &format!("{fork}..{head}")])
        .unwrap_or_default()
        .split('\x1e')
        .filter_map(|record| record.trim_start_matches('\n').split_once('\x1f'))
        .map(|(sha, message)| (sha.to_owned(), message.to_owned()))
        .collect()
}

fn is_harness_text(path: &str) -> bool {
    path.starts_with("harness/skills/") || path.starts_with("harness/agents/") || path.rsplit('/').next().is_some_and(|name| name == "CLAUDE.md" || name == "AGENTS.md")
}

fn is_code(path: &str) -> bool {
    path.ends_with(".rs") && (path.starts_with("src/") || path.starts_with("crates/")) && !test_file::is_test_file(path)
}

fn added_tests(directory: &Path, fork: &str, head: &str) -> usize {
    let diff = git::text(directory, &["diff", "-U0", fork, head, "--", "*.rs"]).unwrap_or_default();
    diff.lines().filter(|line| line.starts_with('+') && !line.starts_with("+++")).filter(|line| matches!(line[1..].trim(), "#[test]" | "#[tokio::test]")).count()
}

fn case_verdicts(directory: &Path, head: &str, changed: &[String]) -> Vec<(String, Option<Verdict>)> {
    let mut cases: Vec<String> = changed
        .iter()
        .filter(|path| path.starts_with(CASES))
        .filter_map(|path| path.strip_suffix(".toml").or_else(|| path.strip_suffix(PROOF_SUFFIX)))
        .map(|stem| format!("{stem}.toml"))
        .filter(|case| git::text(directory, &["cat-file", "-e", &format!("{head}:{case}")]).is_some())
        .collect();
    cases.sort();
    cases.dedup();
    cases
        .into_iter()
        .map(|case| {
            let proof = format!("{}{PROOF_SUFFIX}", case.trim_end_matches(".toml"));
            let verdict = git::text(directory, &["show", &format!("{head}:{proof}")]).and_then(|text| replay_verdict::proven(&text));
            (case, verdict)
        })
        .collect()
}

fn missing(directory: &Path, fork: &str, head: &str) -> Vec<String> {
    let changed: Vec<String> = git::text(directory, &["diff", "--name-only", fork, head]).unwrap_or_default().lines().map(str::to_owned).collect();
    let text = changed.iter().any(|path| is_harness_text(path));
    let code = changed.iter().any(|path| is_code(path));
    let gate = changed.iter().any(|path| path == REGISTRY);
    let mut lacks = Vec::new();
    if text {
        let cases = case_verdicts(directory, head, &changed);
        if !cases.iter().any(|(_, verdict)| *verdict == Some(Verdict::Kept)) {
            let read = if cases.is_empty() {
                "the lane adds no case".to_owned()
            } else {
                cases.iter().map(|(path, verdict)| format!("{path} reads {}", verdict.as_ref().map_or_else(|| "no graded proof".to_owned(), Verdict::line))).collect::<Vec<_>>().join("; ")
            };
            lacks.push(format!("harness text changes need a {CASES}NAME.toml whose NAME{PROOF_SUFFIX} reads kept, written by `north policy-replay case {CASES}NAME.toml --arm base=north@origin/main --arm lane=north@HEAD` ({read})"));
        }
    }
    if code && added_tests(directory, fork, head) == 0 {
        lacks.push("a code guard needs its fails-first test, and the lane adds no #[test]".to_owned());
    }
    if !text && !code && !gate {
        lacks.push("the lane changes no harness text, code or gate, so it carries no guard".to_owned());
    }
    lacks
}

fn enforcing(directory: &Path, revisions: [&str; 2]) -> bool {
    revisions.iter().any(|revision| {
        git::text(directory, &["show", &format!("{revision}:{REGISTRY}")]).is_some_and(|text| gate_shadow::gates(&text).get(SELF).is_some_and(|gate| gate.enforcing))
    })
}

fn record(state: &Path, head: &str, boxes: &[String], lacks: &[String]) {
    let directory = state.join("north/gates");
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let line = serde_json::json!({"gate": SELF, "at": at, "head": head, "would_refuse": !lacks.is_empty(), "boxes": boxes, "lacks": lacks});
    let _ = std::fs::create_dir_all(&directory);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(directory.join(format!("{SELF}.jsonl"))) {
        let _ = writeln!(file, "{line}");
    }
}

pub fn scan(directory: &Path, range: &str, state: &Path, body_of: &dyn Fn(&str) -> Result<String, String>, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{SELF}: expected BASE..HEAD, got {range}");
        return 2;
    };
    let fork = git::merge_base(directory, base, head);
    let mut bodies: HashMap<String, Result<String, String>> = HashMap::new();
    let mut boxes = Vec::new();
    for (sha, message) in commits(directory, &fork, head) {
        let short = &sha[..sha.len().min(SHORT_SHA)];
        for (issue, box_text) in ticked_boxes(&message) {
            match bodies.entry(issue.clone()).or_insert_with(|| body_of(&issue)) {
                Ok(body) => {
                    if open_box(body, &box_text).is_some_and(|text| text.contains(DETECTOR_MARK)) {
                        boxes.push(format!("{issue}: {box_text}"));
                    }
                }
                Err(error) => {
                    let _ = writeln!(out, "{SELF}: {short}: cannot read {issue} to tell whether Box: {box_text} is detector-filed, so it is not judged: {error}");
                }
            }
        }
    }
    if boxes.is_empty() {
        return 0;
    }
    let lacks = missing(directory, &fork, head);
    let enforce = enforcing(directory, [&fork, head]);
    if !enforce {
        record(state, head, &boxes, &lacks);
    }
    if lacks.is_empty() {
        let _ = writeln!(out, "{SELF}: the lane carries the proof for {}", boxes.join("; "));
        return 0;
    }
    let verb = if enforce { "refused" } else { "would refuse (shadow)" };
    let _ = writeln!(out, "{SELF}: {verb}: the lane ticks a detector-filed box ({}) without its proof, so the box stays open:", boxes.join("; "));
    for lack in &lacks {
        let _ = writeln!(out, "  {lack}");
    }
    u8::from(enforce)
}

fn issue_body(issue: &str) -> Result<String, String> {
    let (repo, number) = issue.rsplit_once('#').ok_or_else(|| format!("{issue} is not REPO#N"))?;
    let output = process::run_with_input_within("gh", &["issue", "view", number, "--repo", repo, "--json", "body", "--jq", ".body"], None, GH_LIMIT).ok_or_else(|| format!("gh issue view timed out after {} s", GH_LIMIT.as_secs()))?;
    if !output.status.success() {
        return Err(format!("gh issue view: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn run(directory: &Path, range: &str) -> u8 {
    scan(directory, range, &north_base::platform::records_mirror::state_home(), &issue_body, &mut std::io::stdout())
}

#[cfg(test)]
#[path = "detector_proof_tests.rs"]
mod tests;
