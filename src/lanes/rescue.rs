use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use time::OffsetDateTime;

use super::repo::{find_container, git_checked, git_ok, git_out};
use crate::platform::process;

const USAGE: &str = "Usage: wt-rescue [path] [--dry-run]

Relocate a dirty main checkout's tracked + untracked (non-ignored) changes
into a new worktree/branch under <container>/worktrees/, verify it byte-for-byte,
commit it there, then restore main to a clean working tree.

  (no path)   operate on the container of the current directory
  path        operate on the container reached from this path instead
  --dry-run   print the plan (files, destination) without acting";
const APPLY_LIMIT: Duration = Duration::from_secs(120);

pub(super) struct Rescue {
    pub(super) now: SystemTime,
    pub(super) apply_diff: fn(&Path, &[u8]) -> bool,
}

pub(super) struct Arguments {
    dry_run: bool,
    path: Option<String>,
}

pub(super) enum Finished {
    Done(String),
    Clean,
    Plan(Vec<String>),
    Failed(Vec<String>),
}

struct Changes {
    name_status: Vec<(String, String, Option<String>)>,
    tracked: Vec<String>,
    untracked: Vec<String>,
    diff: Vec<u8>,
}

struct Destination {
    container: PathBuf,
    main_dir: PathBuf,
    rescue_dir: PathBuf,
    branch: String,
    timestamp: String,
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let parsed = match parse(arguments) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => {
            println!("{USAGE}");
            return 0;
        }
        Err(message) => {
            eprintln!("wt-rescue: {message}\n{USAGE}");
            return 2;
        }
    };
    let rescue = Rescue { now: SystemTime::now(), apply_diff };
    let start = parsed.path.map_or_else(|| std::env::current_dir().unwrap_or_default(), PathBuf::from);
    let Ok(start) = start.canonicalize().and_then(|dir| if dir.is_dir() { Ok(dir) } else { Err(std::io::ErrorKind::NotFound.into()) }) else {
        eprintln!("wt-rescue: no such directory '{}'\n{USAGE}", start.display());
        return 2;
    };
    match rescue.rescue(&start, parsed.dry_run) {
        Finished::Done(line) => {
            println!("{line}");
            0
        }
        Finished::Clean => {
            println!("clean, nothing to rescue");
            0
        }
        Finished::Plan(lines) => {
            lines.iter().for_each(|line| println!("{line}"));
            0
        }
        Finished::Failed(lines) => {
            lines.iter().for_each(|line| eprintln!("{line}"));
            1
        }
    }
}

fn parse(arguments: &[String]) -> Result<Option<Arguments>, String> {
    let mut parsed = Arguments { dry_run: false, path: None };
    for argument in arguments {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--dry-run" if parsed.dry_run => return Err("duplicate --dry-run".into()),
            "--dry-run" => parsed.dry_run = true,
            flag if flag.starts_with('-') => return Err(format!("unknown argument '{flag}'")),
            _ if parsed.path.is_some() => return Err("too many arguments".into()),
            path => parsed.path = Some(path.to_string()),
        }
    }
    Ok(Some(parsed))
}

pub(super) fn apply_diff(rescue_dir: &Path, diff: &[u8]) -> bool {
    let directory = rescue_dir.to_string_lossy();
    process::run_with_input_within("git", &["-C", &directory, "apply", "-q", "-"], Some(diff), APPLY_LIMIT)
        .is_some_and(|output| output.status.success())
}

impl Rescue {
    pub(super) fn rescue(&self, start: &Path, dry_run: bool) -> Finished {
        let Some(container) = find_container(start) else {
            return Finished::Failed(vec![format!("wt-rescue: no container found above {} (no <dir>/main/.git)", start.display())]);
        };
        let main_dir = container.join("main");
        match git_out(&main_dir, &["status", "--porcelain"]) {
            None => return Finished::Failed(vec![format!("wt-rescue: could not read the status of {}", main_dir.display())]),
            Some(status) if status.is_empty() => return Finished::Clean,
            Some(_) => {}
        }
        let Some(changes) = gather(&main_dir) else {
            return Finished::Failed(vec!["wt-rescue: could not read main's changes".into()]);
        };
        let destination = self.destination(&container, &main_dir);
        if dry_run {
            return Finished::Plan(plan(&destination, &changes));
        }
        match self.relocate(&destination, &changes) {
            Ok(()) => Finished::Done(format!("rescued -> {} (branch {})", destination.rescue_dir.display(), destination.branch)),
            Err(lines) => Finished::Failed(lines),
        }
    }

    fn destination(&self, container: &Path, main_dir: &Path) -> Destination {
        let moment = OffsetDateTime::from(self.now);
        let timestamp = format!("{:04}{:02}{:02}-{:02}{:02}", moment.year(), u8::from(moment.month()), moment.day(), moment.hour(), moment.minute());
        let mut name = format!("rescue-{timestamp}");
        let mut suffix = 2;
        while container.join("worktrees").join(&name).exists() || git_ok(main_dir, &["show-ref", "--quiet", "--verify", &format!("refs/heads/{name}")]) {
            name = format!("rescue-{timestamp}-{suffix}");
            suffix += 1;
        }
        Destination {
            container: container.to_path_buf(),
            main_dir: main_dir.to_path_buf(),
            rescue_dir: container.join("worktrees").join(&name),
            branch: name,
            timestamp,
        }
    }

    fn relocate(&self, destination: &Destination, changes: &Changes) -> Result<(), Vec<String>> {
        let Destination { container, main_dir, rescue_dir, branch, .. } = destination;
        let failed = |line: String| vec![line];
        fs::create_dir_all(container.join("worktrees")).map_err(|error| failed(error.to_string()))?;
        git_checked(main_dir, &["worktree", "add", "-q", "-b", branch, &rescue_dir.to_string_lossy(), "HEAD"]).map_err(failed)?;
        if !changes.diff.is_empty() && !(self.apply_diff)(rescue_dir, &changes.diff) {
            return Err(failed("wt-rescue: git apply failed".into()));
        }
        for file in &changes.untracked {
            copy_preserving(&main_dir.join(file), &rescue_dir.join(file)).map_err(|error| failed(error.to_string()))?;
        }
        let mismatches = verify(main_dir, rescue_dir, changes);
        if !mismatches.is_empty() {
            let mut lines = mismatches;
            lines.push(format!("wt-rescue: ABORTING — verification failed; main is UNTOUCHED; rescue worktree left at {} for inspection", rescue_dir.display()));
            return Err(lines);
        }
        commit(destination)?;
        restore(main_dir, changes);
        match git_out(main_dir, &["status", "--porcelain"]).filter(|status| !status.is_empty()) {
            Some(status) => Err(vec![
                format!("wt-rescue: main did NOT end clean after restore — rescue worktree ({}, branch {branch}) still holds the verified copy", rescue_dir.display()),
                status,
            ]),
            None => Ok(()),
        }
    }
}

fn gather(main_dir: &Path) -> Option<Changes> {
    let name_status = git_out(main_dir, &["diff", "--name-status", "HEAD"])?;
    let mut parsed = Vec::new();
    let mut tracked = Vec::new();
    for line in name_status.lines().filter(|line| !line.is_empty()) {
        let mut fields = line.split('\t');
        let status = fields.next().unwrap_or_default().to_string();
        let first = fields.next().unwrap_or_default().to_string();
        let second = fields.next().filter(|path| !path.is_empty()).map(str::to_string);
        tracked.push(first.clone());
        tracked.extend(second.clone());
        parsed.push((status, first, second));
    }
    let untracked = git_out(main_dir, &["ls-files", "--others", "--exclude-standard"])?.lines().filter(|line| !line.is_empty()).map(str::to_string).collect();
    let diff = crate::platform::git::run(main_dir, &["diff", "HEAD"]).ok().filter(|output| output.status.success())?.stdout;
    Some(Changes { name_status: parsed, tracked, untracked, diff })
}

fn plan(destination: &Destination, changes: &Changes) -> Vec<String> {
    let mut lines = vec![
        "wt-rescue: DRY RUN — no changes made".to_string(),
        format!("container:   {}", destination.container.display()),
        format!("destination: {} (branch {})", destination.rescue_dir.display(), destination.branch),
        "tracked changes to rescue:".to_string(),
    ];
    lines.extend(changes.tracked.iter().map(|path| format!("  {path}")));
    lines.push("untracked files to rescue:".to_string());
    lines.extend(changes.untracked.iter().map(|path| format!("  {path}")));
    lines
}

fn copy_preserving(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(from, to)?;
    fs::File::open(to)?.set_modified(fs::metadata(from)?.modified()?)
}

fn verify(main_dir: &Path, rescue_dir: &Path, changes: &Changes) -> Vec<String> {
    let mut mismatches = Vec::new();
    for path in changes.tracked.iter().chain(&changes.untracked) {
        let (here, there) = (main_dir.join(path), rescue_dir.join(path));
        match (here.exists(), there.exists()) {
            (true, true) if !matches!((fs::read(&here), fs::read(&there)), (Ok(ours), Ok(theirs)) if ours == theirs) => mismatches.push(format!("wt-rescue: VERIFY MISMATCH: {path}")),
            (true, false) | (false, true) => mismatches.push(format!("wt-rescue: VERIFY MISMATCH (presence): {path}")),
            _ => {}
        }
    }
    mismatches
}

fn commit(destination: &Destination) -> Result<(), Vec<String>> {
    let Destination { container, rescue_dir, branch, timestamp, .. } = destination;
    let left_for_inspection = format!("wt-rescue: rescue worktree left at {} (branch {branch}) for inspection", rescue_dir.display());
    git_checked(rescue_dir, &["add", "-A"]).map_err(|message| vec![message])?;
    if git_ok(rescue_dir, &["diff", "--cached", "--quiet"]) {
        return Err(vec![
            "wt-rescue: nothing differs from HEAD after relocation — main's changes are already committed content; main is UNTOUCHED".into(),
            left_for_inspection,
        ]);
    }
    let message = format!("rescued dirty state from {} main, {timestamp}", container.display());
    if git_checked(rescue_dir, &["commit", "-q", "--no-verify", "-m", &message]).is_err() {
        return Err(vec![
            "wt-rescue: ABORTING — could not commit the relocated copy; main is UNTOUCHED".into(),
            format!("wt-rescue: rescue worktree left at {} (branch {branch}) holding the verified copy", rescue_dir.display()),
        ]);
    }
    Ok(())
}

fn restore(main_dir: &Path, changes: &Changes) {
    for (status, first, second) in &changes.name_status {
        match status.chars().next() {
            Some('A' | 'C') => {
                let added = second.as_deref().unwrap_or(first);
                let _ = git_checked(main_dir, &["rm", "-f", "--cached", "-q", "--", added]);
                let _ = fs::remove_file(main_dir.join(added));
            }
            Some('R') => {
                let renamed = second.as_deref().unwrap_or_default();
                let _ = fs::remove_file(main_dir.join(renamed));
                let _ = git_checked(main_dir, &["reset", "-q", "HEAD", "--", renamed]);
                let _ = git_checked(main_dir, &["checkout", "-q", "HEAD", "--", first]);
            }
            _ => {
                let _ = git_checked(main_dir, &["checkout", "-q", "HEAD", "--", first]);
            }
        }
    }
    for file in &changes.untracked {
        let _ = fs::remove_file(main_dir.join(file));
        let mut directory = main_dir.join(file);
        while directory.pop() && directory.starts_with(main_dir) && fs::remove_dir(&directory).is_ok() {}
    }
}
