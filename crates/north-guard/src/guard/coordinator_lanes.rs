use std::path::PathBuf;
use std::process::Command;

pub(super) fn lane_changes(cwd: &str) -> Option<Vec<String>> {
    let git = |args: &[&str]| -> Option<Vec<String>> {
        let output = Command::new("git").arg("-C").arg(cwd).args(args).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect())
    };
    let base = git(&["merge-base", "HEAD", "origin/main"])?.into_iter().next()?;
    let mut files = git(&["diff", "--name-only", &base])?;
    files.extend(git(&["ls-files", "--others", "--exclude-standard"])?);
    Some(files)
}

pub(super) fn file_bytes(cwd: &str, file: &str) -> Option<Vec<u8>> {
    let output = Command::new("git").args(["-C", cwd, "rev-parse", "--show-toplevel"]).output().ok()?;
    output.status.success().then_some(())?;
    let top = String::from_utf8_lossy(&output.stdout).trim_end().to_owned();
    std::fs::read(PathBuf::from(top).join(file)).ok()
}
