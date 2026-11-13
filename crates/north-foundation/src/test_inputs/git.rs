use std::path::Path;
use std::process::Command;

const IDENTITY: [&str; 6] = ["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false"];

pub fn git_in(dir: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(dir).args(IDENTITY).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub fn init_repo(path: &Path, object_format: Option<&str>) {
    std::fs::create_dir_all(path).unwrap();
    let format = object_format.map(|format| format!("--object-format={format}"));
    let mut args = vec!["init", "-q", "-b", "main"];
    args.extend(format.as_deref());
    git_in(path, &args);
    git_in(path, &["config", "user.name", "fixture"]);
    git_in(path, &["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(path.join("f.txt"), "base\n").unwrap();
    git_in(path, &["add", "f.txt"]);
    git_in(path, &["commit", "-qm", "base"]);
}
