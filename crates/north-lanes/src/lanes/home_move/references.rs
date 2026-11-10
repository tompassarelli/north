//! The config, user-unit and live-process references a home move plan must not touch.

use std::fs;
use std::path::{Path, PathBuf};

const MAX_TEXT: u64 = 1 << 20;

/// What a plan must not touch: the text of every config and user unit on the host, and the paths a
/// live process holds (its working directory, executable and open files).
pub(crate) struct References {
    texts: Vec<String>,
    live: Vec<PathBuf>,
}

impl References {
    pub(super) fn load(home: &Path) -> Self {
        let mut texts = Vec::new();
        read_tree(&home.join(".config"), &mut texts, 0);
        let uid = rustix::process::getuid().as_raw();
        let user = format!("/run/user/{uid}/systemd/user");
        for directory in [
            "/etc/systemd/user",
            "/etc/systemd/system",
            "/run/systemd/system",
            user.as_str(),
        ] {
            read_tree(Path::new(directory), &mut texts, 0);
        }
        let mut live = Vec::new();
        for process in processes(Path::new("/proc")) {
            if let Ok(bytes) = fs::read(process.join("cmdline")) {
                texts.push(String::from_utf8_lossy(&bytes).replace('\0', " "));
            }
            for name in ["cwd", "exe"] {
                if let Ok(path) = fs::read_link(process.join(name)) {
                    live.push(path);
                }
            }
            for descriptor in fs::read_dir(process.join("fd"))
                .into_iter()
                .flatten()
                .flatten()
            {
                if let Ok(path) = fs::read_link(descriptor.path()) {
                    live.push(path);
                }
            }
        }
        Self { texts, live }
    }

    #[cfg(test)]
    pub(super) fn from_parts(texts: Vec<String>, live: Vec<PathBuf>) -> Self {
        Self { texts, live }
    }

    pub(super) fn names(&self, path: &Path, home: &Path) -> bool {
        self.texts.iter().any(|text| names_text(text, path, home))
            || self.live.iter().any(|held| held.starts_with(path))
    }
}

fn read_tree(directory: &Path, texts: &mut Vec<String>, depth: usize) {
    if depth > 8 {
        return;
    }
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            let linked =
                fs::symlink_metadata(&path).is_ok_and(|link| link.file_type().is_symlink());
            if !linked {
                read_tree(&path, texts, depth + 1);
            }
        } else if metadata.is_file()
            && metadata.len() <= MAX_TEXT
            && let Ok(text) = fs::read_to_string(&path)
        {
            texts.push(text);
        }
    }
}

fn processes(proc_root: &Path) -> impl Iterator<Item = PathBuf> {
    fs::read_dir(proc_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|byte| byte.is_ascii_digit())
        })
        .map(|entry| entry.path())
}

fn names_text(text: &str, path: &Path, home: &Path) -> bool {
    let absolute = path.to_string_lossy();
    let relative = path.strip_prefix(home).map(|rest| rest.to_string_lossy());
    let mut forms = vec![absolute.into_owned()];
    if let Ok(relative) = relative
        && !relative.is_empty()
    {
        forms.push(format!("~/{relative}"));
        forms.push(format!("$HOME/{relative}"));
        forms.push(format!("${{HOME}}/{relative}"));
    }
    forms.iter().any(|form| contains(text, form))
}

fn contains(text: &str, needle: &str) -> bool {
    let boundary =
        |character: char| character.is_alphanumeric() || matches!(character, '_' | '-' | '.');
    text.match_indices(needle).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + needle.len()..].chars().next();
        !before.is_some_and(boundary) && !after.is_some_and(boundary)
    })
}
