use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use crate::options::Options;
use crate::{CHILD_VERB, SANDBOX_FAILED, USAGE};

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
    let scratch = tempfile::Builder::new().prefix("north-land-sandbox-").tempdir().map_err(|error| format!("scratch directory: {error}"))?;
    let scratch_path = scratch.path().canonicalize().map_err(|error| format!("scratch directory: {error}"))?;
    let executable = std::env::current_exe().map_err(|error| format!("own executable: {error}"))?;
    let mut child = Command::new(executable);
    child.args(CHILD_VERB).arg("--tree").arg(&tree).arg("--scratch").arg(&scratch_path);
    if let Some(path_set) = &options.enforce {
        let path_set = path_set.canonicalize().map_err(|error| format!("path set {}: {error}", path_set.display()))?;
        child.arg("--enforce").arg(path_set);
    }
    child.arg("--").args(&options.command).env("TMPDIR", &scratch_path);
    // recorder lane: install the seccomp filter in the child and receive its notification descriptor over an inherited socket here
    let status = child.status().map_err(|error| format!("spawn child: {error}"))?;
    let code = exit_code(status);
    if code != 0 && let Some(path_set) = &options.enforce {
        eprintln!("north land sandbox exec: command exited {code} under --enforce {}; a read outside that path set is refused with EACCES, and the recorder that names the path is not built yet", path_set.display());
    }
    Ok(code)
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
