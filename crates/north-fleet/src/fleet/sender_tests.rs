#![cfg(test)]

use std::fs;
use std::path::{Path, PathBuf};

const RUST_SENDS: [&str; 2] = ["\"fleet\", \"send\"", "\"fleet\".into(), \"send\".into()"];
const REACH: usize = 80;

fn files_under(directory: &Path, extension: Option<&str>, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !matches!(path.file_name().and_then(|name| name.to_str()), Some("target" | ".git" | "tests")) {
                files_under(&path, extension, found);
            }
        } else if path.extension().and_then(|found| found.to_str()) == extension {
            found.push(path);
        }
    }
}

fn is_test_source(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with("tests.rs") || name.ends_with("_tests.rs"))
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn no_rust_source_shells_out_to_north_fleet_send_without_naming_its_sender() {
    let root = root();
    let mut sources = Vec::new();
    files_under(&root.join("crates"), Some("rs"), &mut sources);
    files_under(&root.join("src"), Some("rs"), &mut sources);
    let mut bare = Vec::new();
    let mut shell_outs = 0;
    for path in sources.iter().filter(|path| !is_test_source(path)) {
        let Ok(text) = fs::read_to_string(path) else { continue };
        for send in RUST_SENDS {
            for (at, _) in text.match_indices(send) {
                shell_outs += 1;
                let after = &text[at + send.len()..];
                let reach = after.char_indices().nth(REACH).map_or(after.len(), |(end, _)| end);
                if !after[..reach].contains("\"--from\"") && !after[..reach].contains("\"--from\".into()") {
                    bare.push(format!("{}:{}", path.strip_prefix(&root).unwrap_or(path).display(), text[..at].lines().count() + 1));
                }
            }
        }
    }
    assert!(shell_outs >= 10, "the scan found only {shell_outs} shell-outs; it no longer reads the source tree");
    assert!(bare.is_empty(), "north fleet send without --from service:UNIT: {bare:?}");
}

#[test]
fn no_shell_script_runs_north_fleet_send_without_naming_its_sender() {
    let root = root();
    let mut scripts = Vec::new();
    files_under(&root.join("bin"), None, &mut scripts);
    let mut bare = Vec::new();
    let mut shell_outs = 0;
    for path in scripts.iter().filter(|path| !path.to_string_lossy().contains(".test.") && !path.to_string_lossy().contains(".check.") && !path.to_string_lossy().ends_with(".lib.sh")) {
        let Ok(text) = fs::read_to_string(path) else { continue };
        for (number, line) in text.lines().enumerate().filter(|(_, line)| line.contains("fleet send") && !line.trim_start().starts_with("echo") && !line.trim_start().starts_with('#')) {
            shell_outs += 1;
            if !line.contains("--from") {
                bare.push(format!("{}:{}", path.strip_prefix(&root).unwrap_or(path).display(), number + 1));
            }
        }
    }
    assert!(shell_outs >= 1, "the scan found no shell script that sends");
    assert!(bare.is_empty(), "north fleet send without --from service:UNIT: {bare:?}");
}
