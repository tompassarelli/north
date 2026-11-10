use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::super::lane_issue;
use super::evidence::IssueState;
use super::exec::{git, git_with, LIMIT};
use north_base::foundation::clock::Timestamp;

pub(super) const CLOSED_ISSUE_HOURS: f64 = 7.0 * 24.0;
pub(super) const NO_ISSUE_HOURS: f64 = 14.0 * 24.0;
const IDENTITY: [&str; 4] = ["-c", "user.name=north lane gc", "-c", "user.email=lane-gc@north.invalid"];

pub(super) fn window_hours(issues: &[IssueState]) -> Option<f64> {
    if issues.is_empty() {
        Some(NO_ISSUE_HOURS)
    } else if issues.iter().all(|state| *state == IssueState::Closed) {
        Some(CLOSED_ISSUE_HOURS)
    } else {
        None
    }
}

pub(super) struct Lane<'a> {
    pub(super) main: &'a Path,
    pub(super) base: &'a str,
    pub(super) project: &'a str,
    pub(super) worktree: Option<&'a Path>,
    pub(super) branch: &'a str,
    pub(super) tip: &'a str,
    pub(super) issues: &'a [String],
    pub(super) why: &'a str,
}

fn slug(lane: &Lane) -> String {
    let name = if lane.branch.is_empty() { lane.worktree.and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default() } else { lane.branch.to_owned() };
    name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect()
}

fn snapshot(lane: &Lane, worktree: &Path) -> Result<String, String> {
    let status = git(worktree, &["status", "--porcelain", "--untracked-files=normal"]);
    if !status.ok {
        return Err(format!("status failed: {}", status.last_err_line()));
    }
    if status.out.is_empty() {
        return Ok(lane.tip.to_owned());
    }
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    let index = scratch.path().join("index");
    let current = git(worktree, &["rev-parse", "--path-format=absolute", "--git-path", "index"]);
    if current.ok {
        let _ = std::fs::copy(&current.out, &index);
    }
    let indexed = [("GIT_INDEX_FILE", index.as_path())];
    let staged = git_with(worktree, &["add", "-A", "."], LIMIT, &indexed);
    let tree = git_with(worktree, &["write-tree"], LIMIT, &indexed);
    if !staged.ok || !tree.ok {
        return Err(format!("snapshot of uncommitted changes failed: {}", if staged.ok { tree.last_err_line() } else { staged.last_err_line() }));
    }
    let message = format!("north lane gc: uncommitted changes of {}", worktree.display());
    let mut arguments = IDENTITY.to_vec();
    arguments.extend(["commit-tree", &tree.out, "-m", &message]);
    if !lane.tip.is_empty() {
        arguments.extend(["-p", lane.tip]);
    }
    let commit = git(worktree, &arguments);
    if commit.ok { Ok(commit.out) } else { Err(format!("snapshot commit failed: {}", commit.last_err_line())) }
}

fn create(lane: &Lane, head: &str, file: &Path) -> Result<(), String> {
    let reference = format!("refs/north-archive/{}", slug(lane));
    if !git(lane.main, &["update-ref", &reference, head]).ok {
        return Err(format!("cannot point {reference} at {head}"));
    }
    let created = git_with(lane.main, &["bundle", "create", "-q", &file.to_string_lossy(), &reference, &format!("^{}", lane.base)], LIMIT, &[]);
    git(lane.main, &["update-ref", "-d", &reference]);
    if created.ok { Ok(()) } else { Err(format!("bundle failed: {}", created.last_err_line())) }
}

fn bundle(lane: &Lane, head: &str, file: &Path) -> Result<(), String> {
    let fresh = !file.exists();
    if fresh {
        create(lane, head, file)?;
    }
    let verified = git(lane.main, &["bundle", "verify", "-q", &file.to_string_lossy()]);
    let heads = git(lane.main, &["bundle", "list-heads", &file.to_string_lossy()]);
    if !verified.ok || !heads.out.lines().any(|line| line.starts_with(head)) {
        if fresh {
            let _ = std::fs::remove_file(file);
        }
        return Err(format!("bundle {} does not verify", file.display()));
    }
    Ok(())
}

pub(super) fn archive(lane: &Lane, root: &Path, now_ms: i64) -> Result<PathBuf, String> {
    let head = match lane.worktree {
        Some(worktree) => snapshot(lane, worktree)?,
        None => lane.tip.to_owned(),
    };
    let directory = root.join(lane.project);
    std::fs::create_dir_all(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let file = directory.join(format!("{}-{}.bundle", slug(lane), &head[..head.len().min(12)]));
    bundle(lane, &head, &file)?;
    let at = Timestamp::from_unix_millis(now_ms).iso_millis();
    let place = lane.worktree.map_or_else(|| "(branch)".to_owned(), |path| path.display().to_string());
    let issues = if lane.issues.is_empty() { "-".to_owned() } else { lane.issues.join(" ") };
    let line = format!("{at}\t{place}\t{}\t{head}\t{issues}\t{}\t{}\n", if lane.branch.is_empty() { "(detached)" } else { lane.branch }, lane.why, file.display());
    OpenOptions::new().create(true).append(true).open(directory.join("index.tsv")).and_then(|mut index| index.write_all(line.as_bytes())).map_err(|error| error.to_string())?;
    if let Some(worktree) = lane.worktree {
        let removed = git(lane.main, &["worktree", "remove", "--force", &worktree.to_string_lossy()]);
        if !removed.ok {
            return Err(format!("bundled to {}, but worktree remove failed: {}", file.display(), removed.last_err_line()));
        }
    }
    if !lane.branch.is_empty() && !git(lane.main, &["update-ref", "-d", &format!("refs/heads/{}", lane.branch), lane.tip]).ok {
        return Err(format!("bundled to {}, but the branch {} moved", file.display(), lane.branch));
    }
    if !lane.branch.is_empty() {
        lane_issue::forget(lane.main, lane.branch);
    }
    Ok(file)
}
