use std::path::{Component, Path, PathBuf};

pub const PROC: &str = "/proc";

pub fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(part) => result.push(part),
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    result
}

#[cfg(test)]
pub fn proc(rest: &str) -> PathBuf {
    Path::new(PROC).join(rest)
}
