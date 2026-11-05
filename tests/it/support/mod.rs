use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

pub mod template;
pub mod test_env;
pub mod tree;

pub fn fake_claude() -> String {
    static BUILT: OnceLock<String> = OnceLock::new();
    BUILT.get_or_init(|| example("fake_claude", "src/bin/fake_claude.rs")).clone()
}

pub fn push_trace_git() -> String {
    static BUILT: OnceLock<String> = OnceLock::new();
    BUILT.get_or_init(|| example("push_trace_git", "examples/push_trace_git.rs")).clone()
}

fn example(name: &str, source: &str) -> String {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().and_then(Path::parent).unwrap();
    let built = profile.join("examples").join(name);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join(source);
    let modified = |path: &Path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
    if modified(&built).is_some_and(|built| modified(&source).is_some_and(|source| built >= source)) {
        return built.to_string_lossy().into_owned();
    }
    let profile_name = profile.file_name().and_then(|name| name.to_str()).unwrap();
    let status = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--locked", "--offline", "--example", name, "--profile"])
        .arg(if profile_name == "debug" { "dev" } else { profile_name })
        .arg("--manifest-path")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .arg("--target-dir")
        .arg(profile.parent().unwrap())
        .status()
        .unwrap();
    assert!(status.success(), "building {name} failed");
    built.to_string_lossy().into_owned()
}

#[allow(dead_code)]
pub fn fleet_ledger(home: &Path) {
    let dir = home.join(".local/state/threads");
    std::fs::create_dir_all(&dir).unwrap();
    rusqlite::Connection::open(dir.join("threads.db"))
        .unwrap()
        .execute_batch("CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL CHECK(kind IN ('claim','release')), actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;")
        .unwrap();
}
