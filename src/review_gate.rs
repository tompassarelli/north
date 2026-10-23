use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use crate::dep_gate::trailer_lines;
use crate::platform::{git as platform_git, process};

const USAGE: &str = "usage: review-gate id|check BASE HEAD PATH...";
const TRAILER_FORMAT: &str =
    "Reviewed: <reviewer and verdict>; patch-id <id printed by north review-gate id BASE HEAD PATH...>";
static REVIEWED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\AReviewed:\s.*\bpatch-id\s+([0-9a-f]{40,64})\b").unwrap());

#[derive(Debug, Eq, PartialEq)]
enum Verdict {
    Uncovered,
    Reviewed,
    Missing { landing: String },
    Stale { landing: String, reviewed: Vec<String> },
}

fn git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    platform_git::stdout(root, args, |output| {
        format!(
            "git {} failed (exit {}): {}",
            args.iter().find(|arg| !arg.starts_with('-')).unwrap_or(&""),
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    git_bytes(root, args).map(|stdout| String::from_utf8_lossy(&stdout).into_owned())
}

fn commit(root: &Path, revision: &str) -> Result<String, String> {
    git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &format!("{revision}^{{commit}}")],
    )
    .map(|oid| oid.trim().to_owned())
}

fn patch_id(root: &Path, base: &str, head: &str, paths: &[String]) -> Result<Option<String>, String> {
    let head = commit(root, head)?;
    let fork = git(root, &["merge-base", &commit(root, base)?, &head])?;
    let mut args = vec![
        "--literal-pathspecs", "diff", "--no-ext-diff", "--no-textconv", "--no-color",
        "--no-renames", "--no-relative", "--diff-algorithm=myers", "--src-prefix=a/",
        "--dst-prefix=b/", "--binary", "--indent-heuristic", "--inter-hunk-context=0", "-U1", fork.trim(), &head, "--",
    ];
    args.extend(paths.iter().map(String::as_str));
    let diff = git_bytes(root, &args)?;
    if diff.is_empty() {
        return Ok(None);
    }
    let output = process::run_with_input_within(
        "git",
        &["-C", &root.to_string_lossy(), "patch-id", "--verbatim"],
        Some(&diff),
        Duration::from_secs(60),
    )
    .filter(|output| output.status.success())
    .ok_or("git patch-id failed")?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(|id| Some(id.to_owned()))
        .ok_or_else(|| "git patch-id printed no id".into())
}

fn reviewed_ids(messages: &[String]) -> Vec<String> {
    trailer_lines(messages)
        .into_iter()
        .filter_map(|line| REVIEWED.captures(line).map(|c| c[1].to_owned()))
        .collect()
}

fn verdict(root: &Path, base: &str, head: &str, paths: &[String]) -> Result<Verdict, String> {
    let Some(landing) = patch_id(root, base, head, paths)? else {
        return Ok(Verdict::Uncovered);
    };
    let messages: Vec<String> = git(root, &["log", "--format=%B%x00", &format!("{base}..{head}")])?
        .split('\0')
        .map(str::to_owned)
        .collect();
    let reviewed = reviewed_ids(&messages);
    Ok(if reviewed.contains(&landing) {
        Verdict::Reviewed
    } else if reviewed.is_empty() {
        Verdict::Missing { landing }
    } else {
        Verdict::Stale { landing, reviewed }
    })
}

fn refusal(verdict: &Verdict, base: &str, head: &str, paths: &[String]) -> Option<String> {
    let rerun = format!(
        "a fresh reviewer reads the final diff and runs north review-gate id {base} {head} {}; copy its id into the trailer:\n{TRAILER_FORMAT}",
        paths.join(" ")
    );
    match verdict {
        Verdict::Uncovered | Verdict::Reviewed => None,
        Verdict::Missing { landing } => Some(format!(
            "review-gate: covered harness changes carry no Reviewed: trailer with a patch-id; the landing's patch-id is {landing}\n{rerun}\n"
        )),
        Verdict::Stale { landing, reviewed } => Some(format!(
            "review-gate: the review never saw the final diff: Reviewed: patch-id {} does not match the landing's patch-id {landing}\n{rerun}\n",
            reviewed.join(", ")
        )),
    }
}

pub fn run(args: &[String]) -> u8 {
    let [command, base, head, paths @ ..] = args else {
        eprintln!("{USAGE}");
        return 1;
    };
    if paths.is_empty() || !["id", "check"].contains(&command.as_str()) {
        eprintln!("{USAGE}");
        return 1;
    }
    let root = match std::env::current_dir()
        .map_err(|e| e.to_string())
        .and_then(|dir| git(&dir, &["rev-parse", "--show-toplevel"]))
    {
        Ok(root) => PathBuf::from(root.trim_end_matches('\n')),
        Err(e) => {
            eprintln!("review-gate: {e}");
            return 1;
        }
    };
    let outcome = if command == "id" {
        patch_id(&root, base, head, paths).map(|id| match id {
            Some(id) => (0, format!("patch-id {id}\n")),
            None => {
                eprintln!("review-gate: {base}...{head} changes no covered path");
                (0, String::new())
            }
        })
    } else {
        verdict(&root, base, head, paths)
            .map(|verdict| refusal(&verdict, base, head, paths).map_or((0, String::new()), |text| (1, text)))
    };
    match outcome {
        Ok((code, output)) => {
            print!("{output}");
            code
        }
        Err(error) => {
            eprintln!("review-gate: {error}");
            1
        }
    }
}

#[cfg(test)]
#[path = "review_gate_tests.rs"]
mod tests;
