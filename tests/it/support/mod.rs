use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

pub mod tree;

pub fn fake_claude() -> String {
    static BUILT: OnceLock<String> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let exe = std::env::current_exe().unwrap();
            let profile = exe.parent().and_then(Path::parent).unwrap();
            let built = profile.join("examples/fake_claude");
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/fake_claude.rs");
            let modified = |path: &Path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
            if modified(&built).is_some_and(|built| modified(&source).is_some_and(|source| built >= source)) {
                return built.to_string_lossy().into_owned();
            }
            let name = profile.file_name().and_then(|name| name.to_str()).unwrap();
            let status = Command::new(env!("CARGO"))
                .args(["build", "--quiet", "--locked", "--offline", "--example", "fake_claude", "--profile"])
                .arg(if name == "debug" { "dev" } else { name })
                .arg("--manifest-path")
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
                .arg("--target-dir")
                .arg(profile.parent().unwrap())
                .status()
                .unwrap();
            assert!(status.success(), "building fake_claude failed");
            profile.join("examples/fake_claude").to_string_lossy().into_owned()
        })
        .clone()
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
