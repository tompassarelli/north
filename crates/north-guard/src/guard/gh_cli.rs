use std::process::Command;

pub(super) fn gh(cwd: Option<&str>, args: &[&str]) -> Option<String> {
    gh_program("gh", cwd, args)
}

pub(super) fn gh_program(program: &str, cwd: Option<&str>, args: &[&str]) -> Option<String> {
    let mut gh = Command::new(program);
    gh.args(args);
    if let Some(cwd) = cwd {
        gh.current_dir(cwd);
    }
    let out = gh.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
