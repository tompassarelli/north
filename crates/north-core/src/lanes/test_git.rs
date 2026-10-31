use std::path::Path;

use crate::platform::git;

pub(super) fn git_in(dir: &Path, args: &[&str]) -> String {
    let identity = ["-c", "user.name=lanes-test", "-c", "user.email=lanes-test@example.invalid"];
    let all = identity.iter().chain(args).copied().collect::<Vec<_>>();
    let output = git::run(dir, &all).unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub(super) fn init_repo(path: &Path, object_format: Option<&str>) {
    std::fs::create_dir_all(path).unwrap();
    let format = object_format.map(|format| format!("--object-format={format}"));
    let mut args = vec!["init", "-q", "-b", "main"];
    args.extend(format.as_deref());
    git_in(path, &args);
    git_in(path, &["config", "user.name", "lanes-test"]);
    git_in(path, &["config", "user.email", "lanes-test@example.invalid"]);
    std::fs::write(path.join("f.txt"), "base\n").unwrap();
    git_in(path, &["add", "f.txt"]);
    git_in(path, &["commit", "-qm", "base"]);
}

pub(super) fn init_checkout(path: &Path) {
    git_in(path, &["init", "-q", "-b", "main"]);
    git_in(path, &["add", "."]);
    git_in(path, &["commit", "-qm", "fixture"]);
}
