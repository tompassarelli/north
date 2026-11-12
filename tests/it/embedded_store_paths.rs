use std::path::{Path, PathBuf};

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "tests" && name != "target" {
                sources(&path, found);
            }
        } else if name.ends_with(".rs") && !name.ends_with("tests.rs") {
            found.push(path);
        }
    }
}

#[test]
fn no_file_a_shipped_binary_embeds_holds_a_nix_store_hash() {
    let root = crate::support::manifest_dir();
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    sources(&root.join("crates"), &mut files);
    let embed = regex::Regex::new(r#"include_str!\("([^"]+)"\)"#).unwrap();
    let store = regex::Regex::new(r"(?:^|[^0-9a-z])([0-9a-df-np-sv-z]{32})(?:[^0-9a-z]|$)").unwrap();
    let mut named = Vec::new();
    for file in &files {
        let source = std::fs::read_to_string(file).unwrap();
        for capture in embed.captures_iter(&source) {
            let embedded = file.parent().unwrap().join(&capture[1]);
            if let Ok(text) = std::fs::read_to_string(&embedded) {
                named.extend(store.captures_iter(&text).map(|hit| format!("{}: {}", embedded.display(), &hit[1])));
            }
        }
    }
    assert!(named.is_empty(), "embedded files hold Nix store hashes (Nix scans for the hash alone), which become runtime references a host without them cannot root: {named:?}");
}
