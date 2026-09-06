use std::path::{Path, PathBuf};
use std::process::Command;

use crate::agent_catalog::ActivationUnit;
use crate::error::{NorthError, NorthResult};

#[derive(Clone, Debug)]
pub(crate) struct Reference {
    pub name: String,
    pub description: String,
    pub kind: String,
    pub path: PathBuf,
}

impl From<ActivationUnit> for Reference {
    fn from(unit: ActivationUnit) -> Self {
        Self { name: unit.id, description: unit.description, kind: unit.kind, path: unit.source }
    }
}

pub(crate) fn project_files(cwd: &Path) -> NorthResult<Vec<Reference>> {
    let cwd = cwd.canonicalize()?;
    let output = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard", "--deduplicate", "-z", "--", "."])
        .current_dir(&cwd)
        .output()?;
    if !output.status.success() {
        return Err(NorthError::Protocol("Project file references require a Git working directory.".into()));
    }
    let mut paths = output.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty())
        .map(|path| std::str::from_utf8(path).map(str::to_owned)
            .map_err(|_| NorthError::Protocol("A project filename cannot be represented as text.".into())))
        .collect::<NorthResult<Vec<_>>>()?;
    paths.sort();
    paths.dedup();
    Ok(paths.into_iter().filter_map(|name| {
        let path = cwd.join(&name);
        path.is_file().then_some(Reference { name, description: "Project file".into(), kind: "file".into(), path })
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_include_untracked_unicode_and_spaces_but_not_ignored_or_deleted_paths() {
        let directory = tempfile::tempdir().unwrap();
        assert!(Command::new("git").args(["init", "--quiet"]).current_dir(directory.path()).status().unwrap().success());
        std::fs::write(directory.path().join(".gitignore"), "ignored/\n").unwrap();
        std::fs::create_dir(directory.path().join("ignored")).unwrap();
        std::fs::write(directory.path().join("ignored/cache"), "").unwrap();
        std::fs::write(directory.path().join("Å notes.rs"), "").unwrap();
        std::fs::write(directory.path().join("gone.rs"), "").unwrap();
        assert!(Command::new("git").args(["add", "gone.rs"]).current_dir(directory.path()).status().unwrap().success());
        std::fs::remove_file(directory.path().join("gone.rs")).unwrap();
        let files = project_files(directory.path()).unwrap();
        assert_eq!(files.iter().map(|file| file.name.as_str()).collect::<Vec<_>>(), [".gitignore", "Å notes.rs"]);
        assert!(files.iter().all(|file| file.path.is_absolute()));
    }
}
