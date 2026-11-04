use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

const ALLOWED: &[(&str, &str)] = &[
    ("tests/it/land_serve.rs", "a one-line wrapper over the shim that lanes in flight still call"),
    ("tests/it/session_path.rs", "a one-line wrapper over the shim that lanes in flight still call"),
    ("tests/it/safe_push_land_branch.rs", "a one-line wrapper over the shim that lanes in flight still call"),
    ("tests/it/fleet_launch.rs", "a one-line wrapper over the shim that lanes in flight still call"),
    ("tests/it/placement_gate.rs", "a one-line wrapper over the shim that lanes in flight still call"),
    ("tests/it/land_offload.rs", "a one-line wrapper over the shim that lanes in flight still call"),
    ("tests/it/lane_gc_journeys.rs", "a one-line wrapper over the shim that lanes in flight still call"),
    ("tests/it/land_actions.rs", "a one-line wrapper over the shim that lanes in flight still call"),
    ("crates/north-core/src/fleet/install.rs", "production: installs a real runtime"),
    ("crates/north-core/src/catalog/retired_hooks.rs", "production: writes real retired-hook stubs"),
    ("crates/north-core/src/threads/client.rs", "production: spool directory mode"),
    ("crates/north-core/src/ops/private_desktop/start.rs", "production: private desktop startup script"),
    ("crates/north-core/src/ops/runtime/layout.rs", "production: stages the runtime binary"),
    ("crates/north-core/src/ops/reclaim/tests.rs", "data files that are never executed"),
    ("crates/north-core/src/lanes/hardcoded_paths_tests.rs", "a committed file that is never executed"),
    ("crates/north-core/src/fleet/registry/gate_rows.rs", "a directory mode, not a script"),
];

fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "target" && name != ".git") {
                rust_files(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

fn sites(text: &str) -> usize {
    Regex::new(r#"from_mode\(0o7[0-7]{2}\)|fn executable\(path: &Path, |fs::copy\(\s*(?:concat!\(env!\("CARGO_MANIFEST_DIR"\), "/bin|Path::new\(env!\("CARGO_MANIFEST_DIR"\)\)\.join\("bin|env!\("CARGO_BIN_EXE)"#).unwrap().find_iter(text).count()
}

fn offenders(files: &[(String, String)]) -> Vec<String> {
    files
        .iter()
        .filter(|(path, text)| sites(text) > 0 && !ALLOWED.iter().any(|(allowed, _)| allowed == path))
        .map(|(path, _)| path.clone())
        .collect()
}

#[test]
fn a_site_that_makes_a_file_executable_is_flagged_unless_it_is_allowed() {
    let copy = concat!("std::fs::co", "py(env!(\"CARGO_BIN_EXE_north\"), &p);").to_owned();
    let helper = concat!("fn execu", "table(path: &Path, body: &str) {}").to_owned();
    let stub = concat!("fs::set_permissions(&p, fs::Permissions::from_", "mode(0o755)).unwrap();").to_owned();
    let files = [
        ("tests/new_test.rs".to_owned(), stub.clone()),
        ("crates/north-core/src/fleet/install.rs".to_owned(), stub),
        ("tests/copy.rs".to_owned(), copy),
        ("tests/helper.rs".to_owned(), helper),
        ("tests/clean.rs".to_owned(), "shim::write_script(&p, body);".to_owned()),
    ];
    assert_eq!(offenders(&files), vec!["tests/new_test.rs".to_owned(), "tests/copy.rs".to_owned(), "tests/helper.rs".to_owned()]);
}

#[test]
fn no_test_writes_an_executable_stub_without_the_shim_helper() {
    let mut paths = Vec::new();
    for directory in ["crates", "src", "tests"] {
        rust_files(&Path::new(ROOT).join(directory), &mut paths);
    }
    let files: Vec<(String, String)> = paths
        .iter()
        .map(|path| (path.strip_prefix(ROOT).unwrap().to_string_lossy().into_owned(), fs::read_to_string(path).unwrap()))
        .collect();
    assert_eq!(
        offenders(&files),
        Vec::<String>::new(),
        "write executable test stubs with north_core::foundation::shim::write_script or write_executable and copy binaries with copy_executable"
    );
    for (allowed, reason) in ALLOWED {
        let text = files.iter().find(|(path, _)| path == allowed).map(|(_, text)| text.as_str());
        assert!(text.is_some_and(|text| sites(text) > 0), "{allowed} ({reason}) no longer needs its allowance");
    }
}
