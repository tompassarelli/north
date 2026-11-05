use north_base::platform::store_refs;
use std::fs;
use std::path::{Path, PathBuf};

const SCRIPT_LIMIT: u64 = 64 * 1024;

pub struct Reference {
    pub toolchain: String,
    pub file: PathBuf,
    pub path: String,
}

pub fn references(rustup_home: &Path) -> Vec<Reference> {
    let mut toolchains: Vec<PathBuf> = fs::read_dir(rustup_home.join("toolchains")).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    toolchains.sort();
    let mut found = Vec::new();
    for toolchain in toolchains {
        let name = toolchain.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let mut files = Vec::new();
        wrapper_files(&toolchain.join("bin"), &mut files);
        for target in fs::read_dir(toolchain.join("lib/rustlib")).into_iter().flatten().flatten() {
            wrapper_files(&target.path().join("bin"), &mut files);
        }
        for file in files {
            for path in mentions(&file) {
                found.push(Reference { toolchain: name.clone(), file: file.clone(), path });
            }
        }
    }
    found.sort_by(|a, b| (&a.path, &a.file).cmp(&(&b.path, &b.file)));
    found.dedup_by(|a, b| a.path == b.path && a.file == b.file);
    found
}

fn wrapper_files(directory: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => wrapper_files(&path, out),
            Ok(_) => out.push(path),
            Err(_) => {}
        }
    }
}

fn mentions(file: &Path) -> Vec<String> {
    let Ok(meta) = fs::symlink_metadata(file) else {
        return Vec::new();
    };
    if meta.file_type().is_symlink() {
        let target = fs::read_link(file).unwrap_or_default().to_string_lossy().into_owned();
        return store_refs::paths(target.as_bytes());
    }
    match meta.len() <= SCRIPT_LIMIT {
        true => store_refs::paths(&fs::read(file).unwrap_or_default()),
        false => Vec::new(),
    }
}

pub fn missing(references: &[Reference]) -> Vec<&Reference> {
    references.iter().filter(|reference| !Path::new(&reference.path).exists()).collect()
}
