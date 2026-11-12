use std::path::Path;

pub fn is_checkout(root: &Path) -> bool {
    root.join(".git").exists()
}

pub fn under(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs")
                && let Ok(relative) = path.strip_prefix(root)
            {
                found.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    found
}
