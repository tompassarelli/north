use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::{Ctx, Die};


fn rig(root: &Path) -> String {
    let stub = root.join("threads.sh");
    crate::foundation::shim::write_script(&stub, &format!("echo \"$*\" >> {0}/threads.log\ncat {0}/rows.json\n", root.display()));
    stub.display().to_string()
}

fn read(path: &Path) -> Value {
    fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

fn run(root: &Path, stub: &str, now: &str, args: &[&str]) -> Result<String, Die> {
    let arguments: Vec<String> = args.iter().map(|word| (*word).to_owned()).collect();
    Ctx::fixture(root, now, stub).main(&arguments)
}

const ACTIVE: &str = r#"{"gaming":false,"sleeping":false,"idle":false}"#;

fn observe_active(root: &Path, stub: &str, from: i64, minutes: i64) -> Value {
    for minute in 0..=minutes {
        let now = format!("2026-10-13T{:02}:{:02}:00Z", 20 + (from + minute) / 60, (from + minute) % 60);
        run(root, stub, &now, &["observe", ACTIVE]).unwrap();
    }
    read(&root.join("placement/laptop.json"))
}

fn windowed_root(args: &[&str]) -> (tempfile::TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let stub = rig(root.path());
    fs::write(root.path().join("window.json"), json!({"tz": "Etc/GMT-8", "open": "23:00", "close": "10:00"}).to_string()).unwrap();
    fs::write(root.path().join("rows.json"), "[]").unwrap();
    run(root.path(), &stub, "2026-10-13T20:00:00Z", args).unwrap();
    (root, stub)
}

// KEEP(bug): at 3:14 AM on 2026-11-08 laptop-auto drained the laptop 300 s into Tom typing, against his direction that agents may use it while he types; north#834
#[test]
fn an_open_laptop_is_never_drained_by_a_seat_in_use() {
    let (root, stub) = windowed_root(&["open", "until", "06:00", "away"]);
    let held = observe_active(root.path(), &stub, 1, 20);
    assert_eq!(held["state"], "available");
    let still = observe_active(root.path(), &stub, 120, 30);
    assert_eq!(still["state"], "available");
    assert_eq!(still["reason"], held["reason"]);
}
