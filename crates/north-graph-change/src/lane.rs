use std::path::Path;

use north_coverage::hunks::{self, Hunks};
use north_coverage::run_selected::{git, placement};

use crate::workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lane {
    pub name: String,
    pub base: String,
    pub head: String,
}

impl Lane {
    pub fn parse(text: &str) -> Result<Self, String> {
        let (name, range) = text.split_once('=').ok_or_else(|| format!("{text} is not NAME=BASE..HEAD"))?;
        let (base, head) = range.split_once("..").ok_or_else(|| format!("{text} is not NAME=BASE..HEAD"))?;
        if name.is_empty() || base.is_empty() || head.is_empty() {
            return Err(format!("{text} is not NAME=BASE..HEAD"));
        }
        Ok(Self { name: name.to_owned(), base: base.to_owned(), head: head.to_owned() })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Added,
    Deleted,
    Modified,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changed {
    pub path: String,
    pub status: Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddedTest {
    pub file: String,
    pub name: String,
    pub package: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub files: Vec<Changed>,
    pub hunks: Hunks,
    pub added: Vec<AddedTest>,
    pub present: Vec<AddedTest>,
    pub workspace: Workspace,
}

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

impl Change {
    pub fn read(root: &Path, base: &str, head: &str) -> Result<Self, String> {
        let listed = git(root, &["diff", "--name-status", "--no-renames", "--no-color", base, head])?;
        let files: Vec<Changed> = listed
            .lines()
            .filter_map(|line| {
                let (status, path) = line.split_once('\t')?;
                let status = match status.chars().next()? {
                    'A' => Status::Added,
                    'D' => Status::Deleted,
                    _ => Status::Modified,
                };
                Some(Changed { path: path.to_owned(), status })
            })
            .collect();
        let text = git(root, &["diff", "-U0", "--no-renames", "--no-color", "--no-ext-diff", base, head, "--", "*.rs"])?;
        let added = hunks::added_tests(&text).into_iter().flat_map(|(file, names)| {
            let package = placement(root, head, &file).map(|(package, _)| package);
            names.into_iter().map(move |name| AddedTest { file: file.clone(), name, package: package.clone() })
        });
        let paths: Vec<&str> = files.iter().map(|changed| changed.path.as_str()).collect();
        let workspace = Workspace::read(&paths, |revision, path| git(root, &["show", &format!("{revision}:{path}")]), base, head);
        let present = if text.is_empty() { Vec::new() } else { present(root, head)? };
        Ok(Self { files, hunks: hunks::parse(&text), added: added.collect(), present, workspace })
    }

    pub fn touches(&self, path: &str) -> bool {
        self.files.iter().any(|changed| changed.path == path)
    }
}

fn present(root: &Path, head: &str) -> Result<Vec<AddedTest>, String> {
    let tree = git(root, &["diff", "-U0", "--no-renames", "--no-color", "--no-ext-diff", EMPTY_TREE, head, "--", "*.rs"])?;
    Ok(hunks::added_tests(&tree).into_iter().flat_map(|(file, names)| names.into_iter().map(move |name| AddedTest { file: file.clone(), name, package: None })).collect())
}

pub fn carried_since(root: &Path, commit: &str, base: &str, change: &Change) -> Option<Hunks> {
    git(root, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).ok()?;
    let rust: Vec<&str> = change.files.iter().map(|changed| changed.path.as_str()).filter(|path| path.ends_with(".rs")).collect();
    if rust.is_empty() {
        return Some(Hunks::new());
    }
    let mut arguments = vec!["diff", "-U0", "--no-renames", "--no-color", "--no-ext-diff", commit, base, "--"];
    arguments.extend(rust);
    git(root, &arguments).ok().map(|text| hunks::parse(&text))
}

pub fn rust_since(root: &Path, commit: &str, base: &str) -> Option<Hunks> {
    git(root, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).ok()?;
    git(root, &["diff", "-U0", "--no-renames", "--no-color", "--no-ext-diff", commit, base, "--", "*.rs"]).ok().map(|text| hunks::parse(&text))
}
