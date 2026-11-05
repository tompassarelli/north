use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

enum Entry {
    Directory(PathBuf),
    File(PathBuf, Vec<u8>, u32),
}

pub struct Tree(Vec<Entry>);

impl Tree {
    pub fn read(root: &Path) -> Self {
        let mut entries = Vec::new();
        let mut pending = vec![PathBuf::new()];
        while let Some(relative) = pending.pop() {
            for found in fs::read_dir(root.join(&relative)).unwrap() {
                let found = found.unwrap();
                let (kind, path) = (found.file_type().unwrap(), relative.join(found.file_name()));
                if kind.is_dir() {
                    entries.push(Entry::Directory(path.clone()));
                    pending.push(path);
                } else {
                    assert!(kind.is_file(), "{} is neither a file nor a directory", path.display());
                    let mode = found.metadata().unwrap().permissions().mode();
                    entries.push(Entry::File(path.clone(), fs::read(root.join(&path)).unwrap(), mode));
                }
            }
        }
        Self(entries)
    }

    pub fn write(&self, root: &Path) {
        for entry in &self.0 {
            match entry {
                Entry::Directory(path) => fs::create_dir_all(root.join(path)).unwrap(),
                Entry::File(path, bytes, mode) => {
                    let target = root.join(path);
                    fs::create_dir_all(target.parent().unwrap()).unwrap();
                    fs::write(&target, bytes).unwrap();
                    fs::set_permissions(&target, fs::Permissions::from_mode(*mode)).unwrap();
                }
            }
        }
    }
}
