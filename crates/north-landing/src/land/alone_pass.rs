use std::path::Path;

use super::candidate::git;
use super::setting;

pub const LANDINGS_SETTING: &str = "landing.alone-pass-landings";
const LANDINGS_DEFAULT: usize = 5;

pub struct Currency<'a> {
    repo: &'a Path,
    main: &'a str,
    landings: usize,
}

impl<'a> Currency<'a> {
    pub fn at(repo: &'a Path, main: &'a str) -> Self {
        let landings = setting::landing(repo, "refs/remotes/origin/main", LANDINGS_SETTING).trim().parse().unwrap_or(LANDINGS_DEFAULT);
        Self { repo, main, landings }
    }

    pub fn current(&self, checked_on: &str) -> bool {
        checked_on == self.main || landings_since(self.repo, checked_on, self.main).is_some_and(|since| since <= self.landings)
    }
}

pub fn landings_since(repo: &Path, checked_on: &str, main: &str) -> Option<usize> {
    git(repo, &["merge-base", "--is-ancestor", checked_on, main]).ok()?;
    git(repo, &["rev-list", "--count", "--first-parent", &format!("{checked_on}..{main}")]).ok()?.trim().parse().ok()
}

#[cfg(test)]
#[path = "alone_pass_tests.rs"]
mod tests;
