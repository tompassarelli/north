use super::layout::{self, Layout, PROGRAMS, TREE};
use crate::platform::process;
use rustix::fs::{FlockOperation, flock};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

pub(super) struct Failure {
    pub(super) code: u8,
    pub(super) message: String,
}

pub(super) fn failed(code: u8, message: impl Into<String>) -> Failure {
    Failure { code, message: message.into() }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        failed(1, error.to_string())
    }
}

pub(super) fn update(layout: &Layout) -> Result<(), Failure> {
    let Some(_lock) = lock(layout, FlockOperation::NonBlockingLockExclusive)? else {
        println!("north runtime update already running");
        return Ok(());
    };
    let checkout = checkout();
    let (target, runtime) = built_main(layout, &checkout)?;
    let short = &target[..8];
    let previous = fs::read_link(layout.current()).ok();
    if !runtime.join(TREE).is_dir() {
        export_tree(&checkout, &target, &runtime)?;
    }
    let link = layout::link(&target);
    if previous.as_deref() == Some(link.as_path()) {
        println!("north runtime current: {short}");
        return Ok(());
    }
    let synced = sync(layout, previous.as_deref(), &target)?;
    let was = previous.as_deref().and_then(layout::linked_sha).unwrap_or("none");
    println!("{synced}north runtime promoted: {short} (was {was})");
    collect(layout, &target, previous.as_deref())
        .map_err(|error| failed(1, format!("collecting old runtimes: {error}")))
}

pub(super) fn lock(layout: &Layout, operation: FlockOperation) -> Result<Option<File>, Failure> {
    fs::create_dir_all(layout.runtimes())?;
    let lock = File::create(layout.state.join("runtime-update.lock"))?;
    Ok(flock(&lock, operation).is_ok().then_some(lock))
}

pub(super) fn built_main(layout: &Layout, checkout: &Path) -> Result<(String, PathBuf), Failure> {
    git(checkout, &["fetch", "-q"])?;
    let target = git(checkout, &["rev-parse", "--verify", "origin/main^{commit}"])?;
    let runtime = layout.runtime(&target);
    if !layout::has_programs(&runtime) {
        build(checkout, &target, &runtime)?;
    }
    Ok((target, runtime))
}

pub(super) fn checkout() -> PathBuf {
    std::env::var_os("NORTH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| layout::home().join("code/north/main"))
}

fn git<S: AsRef<OsStr>>(checkout: &Path, args: &[S]) -> Result<String, Failure> {
    let output = process::run("git", Some(checkout), args)?;
    if !output.status.success() {
        let shown = args.iter().map(|arg| arg.as_ref().to_string_lossy()).collect::<Vec<_>>();
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(failed(1, format!("git {} failed: {}", shown.join(" "), stderr.trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(super) fn code(status: ExitStatus) -> u8 {
    status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1)
}

struct Lane<'a> {
    checkout: &'a Path,
    path: PathBuf,
}

impl<'a> Lane<'a> {
    fn add(checkout: &'a Path, target: &str) -> Result<Self, Failure> {
        let path = layout::home().join(format!("code/north/worktrees/runtime-{}", &target[..8]));
        let lane = Self { checkout, path };
        lane.remove();
        git(checkout, &[OsStr::new("worktree"), "add".as_ref(), "-q".as_ref(), "--detach".as_ref(), lane.path.as_os_str(), target.as_ref()])?;
        Ok(lane)
    }

    fn remove(&self) {
        let _ = git(self.checkout, &[OsStr::new("worktree"), "remove".as_ref(), "--force".as_ref(), self.path.as_os_str()]);
        let _ = git(self.checkout, &["worktree", "prune"]);
    }
}

impl Drop for Lane<'_> {
    fn drop(&mut self) {
        self.remove();
    }
}

fn build(checkout: &Path, target: &str, runtime: &Path) -> Result<(), Failure> {
    let lane = Lane::add(checkout, target)?;
    let built = layout::home().join(".cache/cargo-target/north");
    let status = Command::new("bun")
        .arg(capacity_helper()?)
        .args(["run", "--class", "heavy", "--owner", "north-runtime", "--timeout-seconds", "1500", "--"])
        .args(["nix", "develop", "-c", "cargo", "build", "--release", "--locked"])
        .current_dir(&lane.path)
        .env("CARGO_TARGET_DIR", &built)
        .status()?;
    if !status.success() {
        return Err(failed(code(status), format!("build of {target} failed: {status}")));
    }
    let release = built.join("release");
    if !layout::has_programs(&release) {
        return Err(failed(75, "build did not run or produced no binary (capacity lease deferred?); runtime unchanged"));
    }
    fs::create_dir_all(runtime)?;
    for program in PROGRAMS {
        install(&release.join(program), &runtime.join(program))?;
    }
    Ok(())
}

fn capacity_helper() -> Result<PathBuf, Failure> {
    let output = process::run("agents", None, &["path", "machine-capacity"])?;
    let skill = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    match skill.parent() {
        Some(skill_dir) if output.status.success() => Ok(skill_dir.join("scripts/machine-capacity.mjs")),
        _ => Err(failed(1, "agents path machine-capacity named no skill")),
    }
}

fn install(from: &Path, to: &Path) -> std::io::Result<()> {
    let staged = to.with_extension("tmp");
    fs::copy(from, &staged)?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    fs::rename(&staged, to)
}

fn export_tree(checkout: &Path, target: &str, runtime: &Path) -> Result<(), Failure> {
    let staging = runtime.join(format!("{TREE}.partial"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let mut archive = Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(["archive", "--format=tar", target])
        .stdout(Stdio::piped())
        .spawn()?;
    let piped = archive.stdout.take().ok_or_else(|| failed(1, "git archive gave no output"))?;
    let extracted = Command::new("tar").arg("-x").arg("-C").arg(&staging).stdin(piped).status()?;
    let archived = archive.wait()?;
    if !(archived.success() && extracted.success()) {
        return Err(failed(1, format!("tree export of {target} failed: git archive {archived}, tar {extracted}")));
    }
    fs::rename(&staging, runtime.join(TREE))?;
    Ok(())
}

fn swap(layout: &Layout, link: &Path) -> std::io::Result<()> {
    let staged = layout.state.join("current.tmp");
    let _ = fs::remove_file(&staged);
    symlink(link, &staged)?;
    fs::rename(&staged, layout.current())
}

fn sync(layout: &Layout, previous: Option<&Path>, target: &str) -> Result<String, Failure> {
    let log = tempfile::Builder::new()
        .prefix(&format!("runtime-sync-{}.", &target[..8]))
        .suffix(".log")
        .tempfile_in(&layout.state)?;
    swap(layout, &layout::link(target))?;
    let ran = log.reopen().and_then(|out| {
        let err = log.reopen()?;
        Command::new("agents").arg("sync").stdout(out).stderr(err).status()
    });
    let status = match ran {
        Ok(status) if status.success() => {
            let mut synced = fs::read_to_string(log.path()).unwrap_or_default();
            if !synced.is_empty() && !synced.ends_with('\n') {
                synced.push('\n');
            }
            return Ok(synced);
        }
        Ok(status) => code(status),
        Err(error) => {
            let _ = fs::write(log.path(), format!("agents sync did not run: {error}\n"));
            127
        }
    };
    if let Some(previous) = previous {
        swap(layout, previous)?;
    }
    let (_, kept) = log.keep().map_err(|error| failed(1, error.to_string()))?;
    let was = previous.map_or_else(|| "<none>".to_owned(), |link| link.display().to_string());
    let message = format!(
        "failed step agents sync on {target}; rolled back to {was}; exit status {status}; local log: {}",
        kept.display()
    );
    alert(&message);
    Err(failed(1, message))
}

fn alert(message: &str) {
    let sent = Command::new("north")
        .args(["fleet", "send", "proxy", &format!("north runtime update: {message}")])
        .status();
    if !sent.is_ok_and(|status| status.success()) {
        eprintln!("north runtime update: rollback alert delivery failed");
    }
}

fn collect(layout: &Layout, target: &str, previous: Option<&Path>) -> std::io::Result<()> {
    let mut listed = Vec::new();
    for entry in fs::read_dir(layout.runtimes())? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            listed.push((entry.metadata()?.modified()?, entry.file_name().to_string_lossy().into_owned()));
        }
    }
    listed.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let names = listed.into_iter().map(|(_, name)| name).collect::<Vec<_>>();
    let mut keep = vec![target];
    keep.extend(previous.and_then(layout::linked_sha));
    let current = fs::canonicalize(layout.current()).ok();
    for name in layout::doomed(&names, &keep) {
        let doomed = layout.runtime(name);
        if fs::canonicalize(&doomed).ok() == current {
            continue;
        }
        process::run("chmod", None, &[OsStr::new("-R"), "u+w".as_ref(), doomed.as_os_str()])?;
        fs::remove_dir_all(&doomed)?;
    }
    Ok(())
}
