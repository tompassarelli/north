use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct Shell {
    pub path: PathBuf,
    pub modified: u64,
}

pub(super) struct Candidate {
    pub agent: String,
    pub session: String,
    pub repo: Option<String>,
    pub last_event: u64,
    pub shells: Vec<Shell>,
}

fn epoch(time: std::io::Result<SystemTime>) -> Option<u64> {
    time.ok()?.duration_since(UNIX_EPOCH).ok().map(|elapsed| elapsed.as_secs())
}

fn children(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir).into_iter().flatten().flatten().map(|entry| entry.path()).collect()
}

fn name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

pub(super) fn repo_of_slug(slug: &str) -> Option<String> {
    let after = slug.split_once("-code-")?.1;
    let end = ["-main", "-worktrees-"].iter().filter_map(|stop| after.find(stop)).min().unwrap_or(after.len());
    Some(after[..end].to_owned()).filter(|repo| !repo.is_empty())
}

pub(super) fn candidates(profiles: &Path, scratch: &Path, now: u64, horizon: u64, silent: u64) -> Vec<Candidate> {
    let mut found = Vec::new();
    for project in children(profiles).into_iter().flat_map(|profile| children(&profile.join("projects"))) {
        let slug = name(&project);
        for session in children(&project).into_iter().filter(|path| path.is_dir()) {
            let tasks = scratch.join(&slug).join(name(&session)).join("tasks");
            for transcript in children(&session.join("subagents")).into_iter().filter(|path| path.extension().is_some_and(|extension| extension == "jsonl")) {
                let Ok(meta) = fs::metadata(&transcript) else { continue };
                let Some(last) = epoch(meta.modified()) else { continue };
                let quiet = now.saturating_sub(last);
                if quiet < silent || quiet > horizon {
                    continue;
                }
                let born = epoch(meta.created()).unwrap_or(0);
                let shells = finished_after(&tasks, born, last);
                if !shells.is_empty() {
                    let agent = transcript.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
                    found.push(Candidate { agent, session: name(&session), repo: repo_of_slug(&slug), last_event: last, shells });
                }
            }
        }
    }
    found
}

fn finished_after(tasks: &Path, born: u64, last: u64) -> Vec<Shell> {
    children(tasks)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "output"))
        .filter_map(|path| {
            let meta = fs::metadata(&path).ok()?;
            let created = epoch(meta.created())?;
            let modified = epoch(meta.modified())?;
            (created >= born && created <= last && modified > last).then_some(Shell { path, modified })
        })
        .collect()
}

pub(super) fn open_files(under: &Path) -> BTreeSet<PathBuf> {
    children(Path::new("/proc"))
        .into_iter()
        .filter(|path| name(path).bytes().all(|byte| byte.is_ascii_digit()))
        .flat_map(|process| children(&process.join("fd")))
        .filter_map(|fd| fs::read_link(fd).ok())
        .filter(|target| target.starts_with(under))
        .collect()
}
