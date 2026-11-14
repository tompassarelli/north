use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use time::OffsetDateTime;

use super::repo::{find_container, git_checked, git_ok, git_out};
use super::rewritten::{self, Followed};
use super::unpushed;
use north_base::platform::process;

const USAGE: &str = "Usage: north lane rescue [path] [--dry-run] [--rewritten]

Relocate a dirty main checkout's tracked + untracked (non-ignored) changes
into a new worktree/branch under <container>/worktrees/, verify it byte-for-byte,
commit it there, then restore main to a clean working tree.

A clean main checkout holding commits origin/main lacks (after fetching origin
main) has them saved on a keep/ branch, reusing one that already contains HEAD,
and main is reset to origin/main; the branch and main are verified afterwards.

With --rewritten, a clean main checkout on main is moved onto origin/main even when
that is not its descendant, as after a history rewrite, only when both hold
the same tree; it refuses a dirty checkout or a differing tree, saves nothing
on a keep/ branch, and touches no file.

  (no path)   operate on the container of the current directory
  path        operate on the container reached from this path instead
  --dry-run   print the plan (files, destination) without acting
  --rewritten follow a rewritten origin/main that keeps the tree";
const APPLY_LIMIT: Duration = Duration::from_secs(120);

pub(super) struct Rescue {
    pub(super) now: SystemTime,
    pub(super) apply_diff: fn(&Path, &[u8]) -> bool,
    pub(super) archive_root: PathBuf,
}

pub(super) struct Arguments {
    dry_run: bool,
    rewritten: bool,
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
    nested: Vec<Nested>,
    diff: Vec<u8>,
}

enum Nested {
    Worktree(String),
    Stale(String),
}

impl Nested {
    fn path(&self) -> &str {
        match self {
            Self::Worktree(path) | Self::Stale(path) => path.trim_end_matches('/'),
        }
    }

    fn name(&self) -> String {
        Path::new(self.path()).file_name().map_or_else(|| "nested".into(), |name| name.to_string_lossy().into_owned())
    }
}

struct Destination {
    container: PathBuf,
    main_dir: PathBuf,
    rescue_dir: PathBuf,
    branch: String,
    timestamp: String,
}

pub fn run(arguments: &[String]) -> u8 {
    let parsed = match parse(arguments) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => {
            println!("{USAGE}");
            return 0;
        }
        Err(message) => {
            eprintln!("lane rescue: {message}\n{USAGE}");
            return 2;
        }
    };
    let archive_root = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("archive/lanes");
    let rescue = Rescue { now: SystemTime::now(), apply_diff, archive_root };
    let start = parsed.path.map_or_else(|| std::env::current_dir().unwrap_or_default(), PathBuf::from);
    let Ok(start) = start.canonicalize().and_then(|dir| if dir.is_dir() { Ok(dir) } else { Err(std::io::ErrorKind::NotFound.into()) }) else {
        eprintln!("lane rescue: no such directory '{}'\n{USAGE}", start.display());
        return 2;
    };
    if parsed.rewritten {
        return follow_rewrite(&start, parsed.dry_run);
    }
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
    let mut parsed = Arguments { dry_run: false, rewritten: false, path: None };
    for argument in arguments {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--dry-run" | "--rewritten" => set_once(&mut parsed, argument)?,
            flag if flag.starts_with('-') => return Err(format!("unknown argument '{flag}'")),
            _ if parsed.path.is_some() => return Err("too many arguments".into()),
            path => parsed.path = Some(path.to_string()),
        }
    }
    Ok(Some(parsed))
}

fn set_once(parsed: &mut Arguments, flag: &str) -> Result<(), String> {
    let field = if flag == "--dry-run" { &mut parsed.dry_run } else { &mut parsed.rewritten };
    if std::mem::replace(field, true) { Err(format!("duplicate {flag}")) } else { Ok(()) }
}

pub(super) fn apply_diff(rescue_dir: &Path, diff: &[u8]) -> bool {
    let directory = rescue_dir.to_string_lossy();
    process::run_with_input_within("git", &["-C", &directory, "apply", "-q", "-"], Some(diff), APPLY_LIMIT)
        .is_some_and(|output| output.status.success())
}

impl Rescue {
    pub(super) fn rescue(&self, start: &Path, dry_run: bool) -> Finished {
        let Some(container) = find_container(start) else {
            return Finished::Failed(vec![format!("lane rescue: no container found above {} (no <dir>/main/.git)", start.display())]);
        };
        let main_dir = container.join("main");
        match git_out(&main_dir, &["--no-optional-locks", "status", "--porcelain"]) {
            None => return Finished::Failed(vec![format!("lane rescue: could not read the status of {}", main_dir.display())]),
            Some(status) if status.is_empty() => return self.rescue_clean(&main_dir, dry_run),
            Some(_) => {}
        }
        let Some(changes) = gather(&main_dir) else {
            return Finished::Failed(vec!["lane rescue: could not read main's changes".into()]);
        };
        let destination = self.destination(&container, &main_dir);
        if dry_run {
            return Finished::Plan(plan(&destination, &changes));
        }
        match self.relocate(&destination, &changes) {
            Ok(()) => Finished::Done(format!("rescued -> {} (branch {}){}", destination.rescue_dir.display(), destination.branch, unpushed::dirty_note(&main_dir))),
            Err(lines) => Finished::Failed(lines),
        }
    }

    fn rescue_clean(&self, main_dir: &Path, dry_run: bool) -> Finished {
        match unpushed::inspect(main_dir) {
            Ok(None) => Finished::Clean,
            Ok(Some(unpushed)) if dry_run => Finished::Plan(unpushed::plan(&unpushed)),
            Ok(Some(unpushed)) => unpushed::apply(main_dir, &unpushed).map_or_else(Finished::Failed, Finished::Done),
            Err(message) => Finished::Failed(vec![format!("lane rescue: {message}; main is UNTOUCHED")]),
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
        for nested in changes.nested.iter().filter(|nested| matches!(nested, Nested::Worktree(_))) {
            let target = container.join("worktrees").join(nested.name());
            if target.exists() {
                return Err(failed(format!("lane rescue: cannot relocate nested worktree {}: {} already exists; main is UNTOUCHED", nested.path(), target.display())));
            }
        }
        require_removable(main_dir, changes)?;
        fs::create_dir_all(container.join("worktrees")).map_err(|error| failed(error.to_string()))?;
        git_checked(main_dir, &["worktree", "add", "-q", "-b", branch, &rescue_dir.to_string_lossy(), "HEAD"]).map_err(failed)?;
        if !changes.diff.is_empty() && !(self.apply_diff)(rescue_dir, &changes.diff) {
            discard(main_dir, rescue_dir, branch);
            return Err(failed("lane rescue: git apply failed; the empty rescue lane was removed; main is UNTOUCHED".into()));
        }
        for file in &changes.untracked {
            if let Err(error) = copy_preserving(&main_dir.join(file), &rescue_dir.join(file)) {
                discard(main_dir, rescue_dir, branch);
                return Err(failed(format!("lane rescue: could not copy {file}: {error}; the rescue lane was removed; main is UNTOUCHED")));
            }
        }
        let mismatches = verify(main_dir, rescue_dir, changes);
        if !mismatches.is_empty() {
            let mut lines = mismatches;
            lines.push(format!("lane rescue: ABORTING — verification failed; main is UNTOUCHED; rescue worktree left at {} for inspection", rescue_dir.display()));
            return Err(lines);
        }
        if changes.tracked.is_empty() && changes.untracked.is_empty() {
            discard(main_dir, rescue_dir, branch);
        } else {
            commit(destination)?;
        }
        restore(main_dir, changes);
        self.relocate_nested(destination, changes)?;
        match git_out(main_dir, &["--no-optional-locks", "status", "--porcelain"]).filter(|status| !status.is_empty()) {
            Some(status) => Err(vec![
                format!("lane rescue: main did NOT end clean after restore — rescue worktree ({}, branch {branch}) still holds the verified copy", rescue_dir.display()),
                status,
            ]),
            None => Ok(()),
        }
    }
}

impl Rescue {
    fn relocate_nested(&self, destination: &Destination, changes: &Changes) -> Result<(), Vec<String>> {
        let Destination { container, main_dir, .. } = destination;
        let mut errors = Vec::new();
        for nested in &changes.nested {
            let from = main_dir.join(nested.path());
            let outcome = match nested {
                Nested::Worktree(_) => git_checked(main_dir, &["worktree", "move", &from.to_string_lossy(), &container.join("worktrees").join(nested.name()).to_string_lossy()]),
                Nested::Stale(_) => self.archive_stale(&from, &destination.timestamp, &nested.name()),
            };
            if let Err(message) = outcome {
                errors.push(format!("lane rescue: could not move nested {}: {message}", nested.path()));
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    fn archive_stale(&self, from: &Path, timestamp: &str, name: &str) -> Result<(), String> {
        fs::create_dir_all(&self.archive_root).map_err(|error| error.to_string())?;
        let mut target = self.archive_root.join(format!("{name}-stale-{timestamp}"));
        let mut suffix = 2;
        while target.exists() {
            target = self.archive_root.join(format!("{name}-stale-{timestamp}-{suffix}"));
            suffix += 1;
        }
        fs::rename(from, &target).map_err(|error| format!("{error} (archiving to {})", target.display()))
    }
}

/// The paths of `changes` that exist in main and cannot be renamed within their directory and back, which is what removing or replacing them needs: a Landlock domain that lets a command create a file in main but refuses its unlink fails here, so the rescue stops before it copies anything.
fn require_removable(main_dir: &Path, changes: &Changes) -> Result<(), Vec<String>> {
    let nested = changes.nested.iter().map(Nested::path);
    let paths = changes.tracked.iter().map(String::as_str).chain(changes.untracked.iter().map(String::as_str)).chain(nested);
    let stuck: Vec<String> = paths.filter(|path| fs::symlink_metadata(main_dir.join(path)).is_ok() && !renames_in_place(&main_dir.join(path))).map(|path| format!("  {path}")).collect();
    if stuck.is_empty() {
        return Ok(());
    }
    let mut lines = vec!["lane rescue: this process cannot remove these paths from main, so a rescue lane could not leave main clean; no rescue lane was created; main is UNTOUCHED".to_string()];
    lines.extend(stuck);
    Err(lines)
}

fn renames_in_place(path: &Path) -> bool {
    let probe = path.with_file_name(format!(".north-rescue-probe-{}", std::process::id()));
    fs::rename(path, &probe).is_ok() && fs::rename(&probe, path).is_ok()
}

fn discard(main_dir: &Path, rescue_dir: &Path, branch: &str) {
    let _ = git_checked(main_dir, &["worktree", "remove", "--force", &rescue_dir.to_string_lossy()]);
    let _ = git_checked(main_dir, &["branch", "-D", branch]);
}

fn registered_worktrees(main_dir: &Path) -> Vec<PathBuf> {
    git_out(main_dir, &["worktree", "list", "--porcelain"])
        .map(|listing| listing.lines().filter_map(|line| line.strip_prefix("worktree ")).map(PathBuf::from).collect())
        .unwrap_or_default()
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
    let listed: Vec<String> = git_out(main_dir, &["ls-files", "--others", "--exclude-standard"])?.lines().filter(|line| !line.is_empty()).map(str::to_string).collect();
    let registered = registered_worktrees(main_dir);
    let (directories, untracked): (Vec<String>, Vec<String>) = listed.into_iter().partition(|path| path.ends_with('/'));
    let nested = directories
        .into_iter()
        .map(|path| {
            let here = main_dir.join(path.trim_end_matches('/')).canonicalize().unwrap_or_default();
            if registered.iter().any(|worktree| worktree.canonicalize().is_ok_and(|worktree| worktree == here)) { Nested::Worktree(path) } else { Nested::Stale(path) }
        })
        .collect();
    let diff = north_base::platform::git::run(main_dir, &["diff", "--binary", "HEAD"]).ok().filter(|output| output.status.success())?.stdout;
    Some(Changes { name_status: parsed, tracked, untracked, nested, diff })
}

fn plan(destination: &Destination, changes: &Changes) -> Vec<String> {
    let mut lines = vec![
        "lane rescue: DRY RUN — no changes made".to_string(),
        format!("container:   {}", destination.container.display()),
        format!("destination: {} (branch {})", destination.rescue_dir.display(), destination.branch),
        "tracked changes to rescue:".to_string(),
    ];
    lines.extend(changes.tracked.iter().map(|path| format!("  {path}")));
    lines.push("untracked files to rescue:".to_string());
    lines.extend(changes.untracked.iter().map(|path| format!("  {path}")));
    lines.push("nested worktrees to relocate:".to_string());
    lines.extend(changes.nested.iter().filter(|nested| matches!(nested, Nested::Worktree(_))).map(|nested| format!("  {} -> {}", nested.path(), destination.container.join("worktrees").join(nested.name()).display())));
    lines.push("stale nested copies to archive:".to_string());
    lines.extend(changes.nested.iter().filter(|nested| matches!(nested, Nested::Stale(_))).map(|nested| format!("  {}", nested.path())));
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
            (true, true) if !matches!((fs::read(&here), fs::read(&there)), (Ok(ours), Ok(theirs)) if ours == theirs) => mismatches.push(format!("lane rescue: VERIFY MISMATCH: {path}")),
            (true, false) | (false, true) => mismatches.push(format!("lane rescue: VERIFY MISMATCH (presence): {path}")),
            _ => {}
        }
    }
    mismatches
}

fn commit(destination: &Destination) -> Result<(), Vec<String>> {
    let Destination { container, rescue_dir, branch, timestamp, .. } = destination;
    let left_for_inspection = format!("lane rescue: rescue worktree left at {} (branch {branch}) for inspection", rescue_dir.display());
    git_checked(rescue_dir, &["add", "-A"]).map_err(|message| vec![message])?;
    if git_ok(rescue_dir, &["diff", "--cached", "--quiet"]) {
        return Err(vec![
            "lane rescue: nothing differs from HEAD after relocation — main's changes are already committed content; main is UNTOUCHED".into(),
            left_for_inspection,
        ]);
    }
    let message = format!("rescued dirty state from {} main, {timestamp}", container.display());
    if git_checked(rescue_dir, &["commit", "-q", "--no-verify", "-m", &message]).is_err() {
        return Err(vec![
            "lane rescue: ABORTING — could not commit the relocated copy; main is UNTOUCHED".into(),
            format!("lane rescue: rescue worktree left at {} (branch {branch}) holding the verified copy", rescue_dir.display()),
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

fn follow_rewrite(start: &Path, dry_run: bool) -> u8 {
    match rewritten::follow(start, dry_run) {
        Ok(Followed::Moved(line)) => println!("lane rescue: moved {line}"),
        Ok(Followed::Plan(line)) => println!("lane rescue: DRY RUN — would move {line}"),
        Ok(Followed::Already) => println!("lane rescue: main already equals origin/main"),
        Err(message) => {
            eprintln!("lane rescue --rewritten: {message}; main is UNTOUCHED");
            return 1;
        }
    }
    0
}
