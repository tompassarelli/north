use std::path::Path;
use std::process::{Command, Stdio};

pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
}

pub fn run(program: &str, args: &[&str]) -> Ran {
    run_in(Path::new("."), program, args)
}

pub fn run_in(dir: &Path, program: &str, args: &[&str]) -> Ran {
    match Command::new(program).current_dir(dir).args(args).stdin(Stdio::null()).output() {
        Ok(output) => Ran { code: output.status.code().unwrap_or(-1), out: String::from_utf8_lossy(&output.stdout).into_owned(), err: String::from_utf8_lossy(&output.stderr).into_owned() },
        Err(error) => Ran { code: 127, out: String::new(), err: format!("{program}: {error}") },
    }
}

pub fn git(dir: &Path, args: &[&str]) -> Ran {
    let dir = dir.to_string_lossy();
    let mut full = vec!["-C", dir.as_ref()];
    full.extend_from_slice(args);
    run("git", &full)
}

pub fn must(ran: Ran, what: &str) -> Result<String, String> {
    if ran.code == 0 { Ok(ran.out.trim().to_owned()) } else { Err(format!("{what} failed (exit {}): {}{}", ran.code, ran.err.trim(), ran.out.trim())) }
}

pub fn sha256(path: &Path) -> Result<String, String> {
    let out = must(run("sha256sum", &[&path.to_string_lossy()]), "sha256sum")?;
    out.split_whitespace().next().map(str::to_owned).ok_or_else(|| format!("sha256sum printed nothing for {}", path.display()))
}
