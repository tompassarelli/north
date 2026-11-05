use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub struct Template {
    root: PathBuf,
    pub base: String,
}

impl Template {
    pub fn get(build: fn(&Path) -> String) -> &'static Template {
        static BUILT: OnceLock<Template> = OnceLock::new();
        BUILT.get_or_init(|| {
            let parent = Path::new(env!("CARGO_TARGET_TMPDIR"));
            fs::create_dir_all(parent).unwrap();
            remove_stale(parent);
            let root = parent.join(format!("land-serve-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            let base = build(&root);
            Template { root, base }
        })
    }

    pub fn copy_into(&self, root: &Path) {
        copy_tree(&self.root, root);
        let config = root.join("proj/main/.git/config");
        let text = fs::read_to_string(&config).unwrap();
        fs::write(&config, text.replace(self.root.to_str().unwrap(), root.to_str().unwrap())).unwrap();
    }
}

fn remove_stale(parent: &Path) {
    for entry in fs::read_dir(parent).unwrap().filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|name| name.strip_prefix("land-serve-")) else { continue };
        if pid.parse::<u32>().is_ok() && !Path::new("/proc").join(pid).exists() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_tree(&source, &target);
        } else if kind.is_symlink() {
            symlink(fs::read_link(&source).unwrap(), &target).unwrap();
        } else {
            fs::copy(&source, &target).unwrap();
        }
    }
}
