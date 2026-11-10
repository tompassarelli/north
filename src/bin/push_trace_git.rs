use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, exit};

fn main() {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let git = real_git();
    if !arguments.iter().any(|argument| argument == "push") {
        let error = Command::new(&git).args(&arguments).exec();
        panic!("exec {}: {error}", git.display());
    }
    let trace = PathBuf::from(std::env::var_os("LAND_TRACE").unwrap_or_default());
    let joined = arguments.iter().map(|argument| argument.to_string_lossy()).collect::<Vec<_>>().join(" ");
    let token = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty()).unwrap_or_else(|| "none".to_owned());
    let line = format!("{joined} token={} app={}\n", token("GH_TOKEN"), token("NORTH_LAND_PUSH_TOKEN"));
    OpenOptions::new().create(true).append(true).open(trace.join("pushes")).and_then(|mut file| file.write_all(line.as_bytes())).unwrap();
    let kill = trace.join("kill-after-push");
    if !kill.exists() {
        let error = Command::new(&git).args(&arguments).exec();
        panic!("exec {}: {error}", git.display());
    }
    let status = Command::new(&git).args(&arguments).status().unwrap();
    let _ = fs::remove_file(&kill);
    let _ = Command::new("kill").args(["-9", &std::os::unix::process::parent_id().to_string()]).status();
    exit(code(status));
}

fn code(status: ExitStatus) -> i32 {
    status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

fn real_git() -> PathBuf {
    let own = std::env::current_exe().and_then(fs::canonicalize).ok();
    std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|directory| directory.join("git"))
        .find(|candidate| is_executable(candidate) && fs::canonicalize(candidate).ok() != own)
        .expect("no git on PATH besides this trace wrapper")
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}
