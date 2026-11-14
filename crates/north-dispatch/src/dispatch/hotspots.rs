//! The paths a box names, in its own line or else on its issue's `Files:` line, which one dispatch run
//! staffs to a single lane at a time so two workers never edit the same module at once.
use super::facets::declared;
use super::issues::Issue;
use regex::Regex;
use std::sync::LazyLock;

static PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.*-]+)+/?$|^[A-Za-z0-9_-]+\.(?:rs|toml|md|ts|mjs|js|json|sh|nix|lua|ya?ml)$").expect("path pattern"));

/// The paths held by the boxes this run launched or found running, each with the lane that holds it.
#[derive(Default)]
pub struct Claims {
    held: Vec<(String, String)>,
    predicted: Vec<(String, String)>,
}

impl Claims {
    /// The first of `paths` that overlaps a held path, one being the other or inside it, with its holder.
    pub fn conflict(&self, paths: &[String]) -> Option<(String, &str)> {
        paths.iter().find_map(|path| self.held.iter().find(|(held, _)| overlaps(path, held)).map(|(_, holder)| (path.clone(), holder.as_str())))
    }

    /// The first of `paths` that overlaps a predicted write of a running session, with that session.
    pub fn predicted_conflict(&self, paths: &[String]) -> Option<(String, &str)> {
        paths.iter().find_map(|path| self.predicted.iter().find(|(held, _)| overlaps(path, held)).map(|(_, holder)| (path.clone(), holder.as_str())))
    }

    pub fn take_predicted(&mut self, paths: &[String], holder: &str) {
        self.predicted.extend(paths.iter().map(|path| (path.clone(), holder.to_owned())));
    }

    pub fn take(&mut self, paths: &[String], holder: &str) {
        self.held.extend(paths.iter().map(|path| (path.clone(), holder.to_owned())));
    }
}

pub fn hold_reason(path: &str, holder: &str) -> String {
    format!("held for {holder}: both name {path}, and two lanes never edit one module at once")
}

pub fn predicted_reason(path: &str, holder: &str) -> String {
    format!("held for {holder}: predicted write set names {path}")
}

/// The paths the box `box_text` of `issue` names, else those its issue's `Files:` line names; the whole issue's when no box is named.
pub fn paths(issue: &Issue, box_text: Option<&str>) -> Vec<String> {
    let own = box_text.map(named).unwrap_or_default();
    if !own.is_empty() {
        return own;
    }
    declared(issue, "Files").map(named).unwrap_or_default()
}

pub(super) fn named(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, ',' | '`' | '(' | ')' | '\'' | '"'))
        .map(|word| word.trim_end_matches(['.', ';', ':']))
        .filter(|word| PATH.is_match(word) && !word.starts_with("http") && !word.starts_with("origin/"))
        .map(|word| word.trim_end_matches('/').to_owned())
        .collect()
}

fn overlaps(a: &str, b: &str) -> bool {
    let within = |inner: &str, outer: &str| inner.strip_prefix(outer).is_some_and(|rest| rest.starts_with('/'));
    a == b || within(a, b) || within(b, a)
}
