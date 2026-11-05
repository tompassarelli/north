use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use north_landing::stub_exec_gate::{ALLOWLIST, ROOTS, allowlist, problems};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

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

#[test]
fn no_test_writes_an_executable_stub_without_the_shim_helper() {
    let root = Path::new(ROOT);
    let mut paths = Vec::new();
    for directory in ROOTS {
        rust_files(&root.join(directory), &mut paths);
    }
    let files: BTreeMap<String, String> = paths
        .iter()
        .map(|path| (path.strip_prefix(root).unwrap().to_string_lossy().into_owned(), fs::read_to_string(path).unwrap()))
        .collect();
    let allowed = allowlist(&fs::read(root.join(ALLOWLIST)).unwrap()).unwrap();
    assert_eq!(problems(&files, &allowed), Vec::<String>::new());
}
