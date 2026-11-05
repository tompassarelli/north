use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub struct Template {
    name: &'static str,
    built: OnceLock<PathBuf>,
}

impl Template {
    pub const fn new(name: &'static str) -> Self {
        Self { name, built: OnceLock::new() }
    }

    pub fn copy_to(&self, root: &Path, build: impl FnOnce(&Path)) {
        let template = self.built.get_or_init(|| {
            let parent = Path::new(env!("CARGO_TARGET_TMPDIR")).join("templates");
            sweep(&parent);
            let template = parent.join(format!("{}-{}", self.name, std::process::id()));
            if template.exists() {
                fs::remove_dir_all(&template).unwrap();
            }
            fs::create_dir_all(&template).unwrap();
            build(&template);
            template
        });
        copy_tree(template, root);
    }
}

fn sweep(parent: &Path) {
    for entry in fs::read_dir(parent).into_iter().flatten().flatten() {
        let owner = entry.file_name().to_string_lossy().rsplit_once('-').and_then(|(_, pid)| pid.parse::<u32>().ok());
        if owner.is_some_and(|pid| pid != std::process::id() && !Path::new(&format!("/proc/{pid}")).exists()) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

pub fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type().unwrap();
        if kind.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(&source).unwrap(), &target).unwrap();
        } else if kind.is_dir() {
            copy_tree(&source, &target);
        } else {
            fs::copy(&source, &target).unwrap();
        }
    }
}
