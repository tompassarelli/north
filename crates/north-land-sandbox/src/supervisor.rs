use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use nix::unistd::Pid;
use north_base::verdict_cache::path_set::{self, Entry};

use crate::classify::Roots;
use crate::options::Options;
use crate::recorder::{self, Recorder};
use crate::{CHILD_VERB, SANDBOX_FAILED, USAGE, fence};

pub fn run(arguments: &[String]) -> u8 {
    let options = match Options::parse(arguments) {
        Ok(options) => options,
        Err(reason) => {
            eprintln!("north land sandbox exec: {reason}\nusage: {USAGE}");
            return 64;
        }
    };
    match supervise(&options) {
        Ok(code) => code,
        Err(reason) => {
            eprintln!("north land sandbox exec: {reason}");
            SANDBOX_FAILED
        }
    }
}

fn supervise(options: &Options) -> Result<u8, String> {
    let tree = match &options.tree {
        Some(tree) => tree.clone(),
        None => git_toplevel()?,
    };
    let tree = tree.canonicalize().map_err(|error| format!("tree {}: {error}", tree.display()))?;
    let scratch = scratch_directory()?;
    let scratch_path = scratch.path().canonicalize().map_err(|error| format!("scratch directory: {error}"))?;
    let executable = std::env::current_exe().map_err(|error| format!("own executable: {error}"))?;
    let mut child = Command::new(executable);
    child.args(CHILD_VERB).arg("--tree").arg(&tree).arg("--scratch").arg(&scratch_path);
    let mut allowed = None;
    if let Some(path_set) = &options.enforce {
        let path_set = path_set.canonicalize().map_err(|error| format!("path set {}: {error}", path_set.display()))?;
        allowed = Some(read_path_set(&path_set)?);
        child.arg("--enforce").arg(path_set);
    }
    let traced = options.record.is_some() || options.enforce.is_some();
    if traced {
        child.arg("--trace");
    }
    child.arg("--").args(&options.command).env("TMPDIR", &scratch_path);
    if !traced {
        return Ok(exit_code(child.status().map_err(|error| format!("spawn child: {error}"))?));
    }
    let target = fence::target_dir(&tree);
    let roots = Roots { target: target.canonicalize().unwrap_or(target), tree, scratch: scratch_path, program: options.command.first().and_then(|program| fence::resolved(Path::new(program))) };
    let mut recorder = Recorder::new(roots, allowed);
    let process = child.spawn().map_err(|error| format!("spawn child: {error}"))?;
    let pid = Pid::from_raw(i32::try_from(process.id()).map_err(|error| error.to_string())?);
    let mut code = match recorder::follow(&mut recorder, pid) {
        Ok(code) => code,
        Err(reason) => {
            recorder.mark_unreusable(reason.clone());
            eprintln!("north land sandbox exec: {reason}");
            SANDBOX_FAILED
        }
    };
    let report = recorder.finish();
    if let Some(record) = &options.record {
        std::fs::write(record, path_set::render(&report.entries)).map_err(|error| format!("record {}: {error}", record.display()))?;
    }
    for path in &report.refused {
        eprintln!("sandbox refused read of {path}");
    }
    if code == 0 && !report.refused.is_empty() {
        code = 1;
    }
    Ok(code)
}

fn scratch_directory() -> Result<tempfile::TempDir, String> {
    let short = Path::new("/tmp");
    let parent = if short.is_dir() { short.to_owned() } else { std::env::temp_dir() };
    tempfile::Builder::new().prefix("n-").tempdir_in(parent).map_err(|error| format!("scratch directory: {error}"))
}

fn read_path_set(file: &Path) -> Result<Vec<Entry>, String> {
    let text = std::fs::read_to_string(file).map_err(|error| format!("path set {}: {error}", file.display()))?;
    path_set::parse(&text).map_err(|error| format!("{}: {error}", file.display()))
}

fn exit_code(status: ExitStatus) -> u8 {
    match (status.code(), status.signal()) {
        (Some(code), _) => u8::try_from(code & 0xff).unwrap_or(1),
        (None, Some(signal)) => u8::try_from(128 + signal).unwrap_or(SANDBOX_FAILED),
        (None, None) => SANDBOX_FAILED,
    }
}

fn git_toplevel() -> Result<PathBuf, String> {
    let output = Command::new("git").args(["rev-parse", "--show-toplevel"]).output().map_err(|error| format!("git rev-parse: {error}"))?;
    if !output.status.success() {
        return Err("no --tree and the working directory is not in a git tree".into());
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim_end()))
}
