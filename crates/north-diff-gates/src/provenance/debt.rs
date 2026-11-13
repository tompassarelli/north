use super::record::{fields, records};
use crate::foundation::clock;
use crate::platform::{git as platform_git, github};
use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}
fn repos(args: &[String]) -> Vec<PathBuf> {
    if !args.is_empty() {
        return args.iter().map(PathBuf::from).collect();
    }
    let mut result: Vec<_> = std::fs::read_dir(home().join("code"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path().join("main"))
        .filter(|p| p.exists())
        .collect();
    result.sort();
    result
}
fn repo_name(repo: &Path) -> String {
    let path = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    let path = if path.file_name().is_some_and(|n| n == "main") {
        path.parent().unwrap_or(&path)
    } else {
        &path
    };
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
fn issue_due(value: &str) -> Result<Option<bool>, String> {
    let Some(m) = re!(r"\Aclosed\s+([\w.-]+)#(\d+)\z").captures(value) else {
        return Ok(None);
    };
    let out = github::run(&[
        "issue",
        "view",
        &m[2],
        "-R",
        &format!("tompassarelli/{}", &m[1]),
        "--json",
        "state",
    ])
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Ok(Some(false));
    }
    let value: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let state = value.get("state").ok_or("gh response has no state")?;
    Ok(Some(state == "CLOSED"))
}
fn due(value: &str, event: bool) -> Result<bool, String> {
    Ok(issue_due(value)?.unwrap_or_else(|| clock::parse_python_date(value).map_or(event, |d| d <= clock::local_today())))
}
pub(super) fn debt(paths: &[String], only_due: bool) -> Result<(), String> {
    for repo in repos(paths) {
        if !repo.is_dir() {
            return Err(format!("repository does not exist: {}", repo.display()));
        }
        let output = platform_git::run(&repo, &["log", "origin/main", "-z", "--format=%H%x00%B"])
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            continue;
        }
        let log = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<_> = log.split('\0').collect();
        let commits: Vec<_> = parts.chunks_exact(2).map(|v| (v[0], v[1])).collect();
        let paid: Vec<_> = commits
            .iter()
            .flat_map(|(_, body)| records(body, "Target-paid:"))
            .filter_map(|v| {
                re!(r"^[0-9a-f]{7,40}")
                    .find(v)
                    .map(|m| m.as_str().to_owned())
            })
            .collect();
        for (sha, body) in commits {
            if paid.iter().any(|p| sha.starts_with(p)) {
                continue;
            }
            for value in records(body, "Target-debt:") {
                let (bar, named, _) = fields(value);
                let revisit = named.get("revisit").copied().unwrap_or("");
                if !only_due || due(revisit, true)? {
                    println!(
                        "{}\t{}\t{bar}\t{}\t{}\t{revisit}",
                        repo_name(&repo),
                        &sha[..sha.len().min(12)],
                        named.get("measured").unwrap_or(&""),
                        named.get("owner").unwrap_or(&"")
                    );
                }
            }
        }
    }
    Ok(())
}
