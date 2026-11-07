use std::path::Path;

use super::repo::{git_checked, git_ok, git_out};

pub(super) struct Unpushed {
    head: String,
    target: String,
    commits: Vec<String>,
    keep: String,
    create: bool,
}

pub(super) fn inspect(main_dir: &Path) -> Result<Option<Unpushed>, String> {
    if git_out(main_dir, &["symbolic-ref", "--short", "HEAD"]).as_deref() != Some("main")
        || !git_ok(main_dir, &["remote", "get-url", "origin"])
    {
        return Ok(None);
    }
    git_checked(main_dir, &["fetch", "-q", "origin", "main"])?;
    let commits = git_out(main_dir, &["log", "--oneline", "origin/main..HEAD"])
        .ok_or("could not list the commits ahead of origin/main")?;
    if commits.is_empty() {
        return Ok(None);
    }
    let head = git_out(main_dir, &["rev-parse", "HEAD"]).ok_or("could not read HEAD")?;
    let target =
        git_out(main_dir, &["rev-parse", "origin/main"]).ok_or("could not read origin/main")?;
    let (keep, create) = keep_branch(main_dir, &head);
    Ok(Some(Unpushed {
        head,
        target,
        commits: commits.lines().map(str::to_string).collect(),
        keep,
        create,
    }))
}

fn keep_branch(main_dir: &Path, head: &str) -> (String, bool) {
    let containing = |prefix: &str| {
        git_out(
            main_dir,
            &[
                "for-each-ref",
                "--contains",
                head,
                "--format=%(refname:short)",
                prefix,
            ],
        )
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>()
    };
    if let Some(existing) = containing("refs/heads/keep/").into_iter().next() {
        return (existing, false);
    }
    let slug = containing("refs/heads/")
        .into_iter()
        .find(|branch| branch != "main")
        .map(|branch| sanitize(&branch))
        .unwrap_or_else(|| "rescued".into());
    (format!("keep/{slug}-{}", &head[..9.min(head.len())]), true)
}

fn sanitize(branch: &str) -> String {
    branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub(super) fn plan(unpushed: &Unpushed) -> Vec<String> {
    let mut lines = vec![
        "lane rescue: DRY RUN — no changes made".to_string(),
        format!("commits ahead of origin/main ({}):", unpushed.commits.len()),
    ];
    lines.extend(unpushed.commits.iter().map(|commit| format!("  {commit}")));
    let action = if unpushed.create { "create" } else { "reuse" };
    lines.push(format!("keep branch: {} ({action})", unpushed.keep));
    lines.push(format!("main moves to: {}", unpushed.target));
    lines
}

pub(super) fn apply(main_dir: &Path, unpushed: &Unpushed) -> Result<String, Vec<String>> {
    let fail =
        |line: String| Err(vec![format!("lane rescue: {line}; main is UNTOUCHED")]);
    if unpushed.create
        && let Err(message) = git_checked(main_dir, &["branch", &unpushed.keep, &unpushed.head])
    {
        return fail(format!(
            "could not save HEAD on {}: {message}",
            unpushed.keep
        ));
    }
    if let Err(message) = git_checked(main_dir, &["reset", "-q", "--keep", &unpushed.target]) {
        return fail(format!("could not move main to origin/main: {message}"));
    }
    let saved = git_ok(
        main_dir,
        &[
            "merge-base",
            "--is-ancestor",
            &unpushed.head,
            &unpushed.keep,
        ],
    );
    let moved =
        git_out(main_dir, &["rev-parse", "HEAD"]).as_deref() == Some(unpushed.target.as_str());
    if !saved || !moved {
        return Err(vec![format!(
            "lane rescue: verification failed (keep branch {} holds the old HEAD {}: {saved}; main equals origin/main: {moved})",
            unpushed.keep, unpushed.head
        )]);
    }
    Ok(format!(
        "rescued unpushed commits -> {} (main now {})",
        unpushed.keep,
        &unpushed.target[..12.min(unpushed.target.len())]
    ))
}

pub(super) fn dirty_note(main_dir: &Path) -> String {
    match inspect(main_dir) {
        Ok(Some(unpushed)) => format!(
            "; main still holds {} commits origin/main lacks, run lane rescue again to move them",
            unpushed.commits.len()
        ),
        _ => String::new(),
    }
}
