use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use nix::unistd::Pid;
use north_base::verdict_cache::path_set::{self, Entry, Kind};

use crate::classify::{Roots, TARGET_LABEL};
use crate::options::Options;
use crate::recorder::{self, Recorder};
use crate::{CHILD_VERB, RECORD_ENV, SANDBOX_FAILED, USAGE, fence, not_inputs};

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
    let record = options.record_file();
    let traced = record.is_some() || options.enforce.is_some();
    if traced {
        child.arg("--trace");
    }
    if let Some(record) = &record {
        child.env(RECORD_ENV, record);
    }
    child.arg("--").args(&options.command).env("TMPDIR", &scratch_path);
    if !traced {
        return Ok(exit_code(child.status().map_err(|error| format!("spawn child: {error}"))?));
    }
    let target = fence::target_dir(&tree);
    let target = target.canonicalize().unwrap_or(target);
    let program = options.command.first().and_then(|program| fence::resolved(Path::new(program)));
    let own = program.as_deref().and_then(|program| program_entry(program, &target, &tree));
    let roots = Roots { target, tree, scratch: scratch_path, program, library_dirs: not_inputs::library_dirs(std::env::var_os("LD_LIBRARY_PATH")) };
    let mut recorder = Recorder::new(roots, allowed);
    let process = child.spawn().map_err(|error| format!("spawn child: {error}"))?;
    let pid = Pid::from_raw(i32::try_from(process.id()).map_err(|error| error.to_string())?);
    let root = u32::try_from(pid.as_raw()).map_err(|error| error.to_string())?;
    let mut code = match recorder::follow(&mut recorder, pid) {
        Ok(code) => code,
        Err(reason) => {
            recorder.mark_unreusable(reason.clone());
            recorder.mark_incomplete(reason.clone());
            eprintln!("north land sandbox exec: {reason}");
            SANDBOX_FAILED
        }
    };
    let mut report = recorder.finish();
    report.entries.extend(own);
    if let Some(record) = &record {
        write_record(record, options.exec_log, root, &report)?;
    }
    for path in &report.refused {
        eprintln!("sandbox refused read of {path}");
    }
    if code == 0 && !report.refused.is_empty() {
        code = 1;
    }
    Ok(code)
}

fn write_record(record: &Path, exec_log_too: bool, root: u32, report: &recorder::Report) -> Result<(), String> {
    std::fs::write(record, path_set::render(&report.entries)).map_err(|error| format!("record {}: {error}", record.display()))?;
    if exec_log_too {
        let path = PathBuf::from(format!("{}.exec", record.display()));
        std::fs::write(&path, exec_log(root, report)).map_err(|error| format!("exec log {}: {error}", path.display()))?;
    }
    Ok(())
}

fn scratch_directory() -> Result<tempfile::TempDir, String> {
    let short = Path::new("/tmp");
    let parent = if short.is_dir() { short.to_owned() } else { std::env::temp_dir() };
    tempfile::Builder::new().prefix("n-").tempdir_in(parent).map_err(|error| format!("scratch directory: {error}"))
}

fn exec_log(root: u32, report: &recorder::Report) -> String {
    let mut text = format!("root\t{root}\n");
    for reason in &report.incomplete {
        text.push_str(&format!("incomplete\t{reason}\n"));
    }
    for exec in &report.execs {
        text.push_str(&format!("exec\t{}\t{}\t{}\t{}\n", exec.pid, exec.device, exec.inode, exec.path));
    }
    text
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

fn program_entry(program: &Path, target: &Path, tree: &Path) -> Option<Entry> {
    let path = match program.strip_prefix(target) {
        Ok(relative) => format!("{TARGET_LABEL}/{}", relative.to_str()?),
        Err(_) => program.strip_prefix(tree).ok()?.to_str()?.to_owned(),
    };
    Some(Entry { kind: Kind::Exec, path })
}
