use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use north_base::cites;

use super::doc_paths::{Tree, broken_links, missing_paths};
use super::{Range, git};

const DOCUMENTS: [&str; 3] = ["README.md", "AGENTS.md", "docs/*.md"];

fn documents(directory: &Path, revision: &str) -> Option<BTreeMap<String, String>> {
    let mut arguments = vec!["grep", "-z", "-I", "--no-color", "-e", "", revision, "--"];
    arguments.extend(DOCUMENTS);
    let output = git::bytes(directory, &arguments);
    let output = match output {
        Some(output) => output,
        None if git::text(directory, &["rev-parse", "--verify", "--quiet", revision]).is_some() => Vec::new(),
        None => return None,
    };
    let prefix = format!("{revision}:");
    let mut documents: BTreeMap<String, String> = BTreeMap::new();
    for line in output.split(|&byte| byte == b'\n') {
        let line = String::from_utf8_lossy(line);
        let Some((name, text)) = line.split_once('\0') else {
            continue;
        };
        let name = name.strip_prefix(&prefix).unwrap_or(name);
        let entry = documents.entry(name.to_owned()).or_default();
        entry.push_str(text);
        entry.push('\n');
    }
    Some(documents)
}

struct Revision<'a> {
    directory: &'a Path,
    revision: &'a str,
    tree: &'a Tree,
}

impl Revision<'_> {
    fn show(&self, path: &str) -> Option<String> {
        git::text(self.directory, &["show", &format!("{}:{path}", self.revision)])
    }
}

impl cites::Tree for Revision<'_> {
    fn exists(&self, path: &str) -> bool {
        self.tree.contains(path)
    }

    fn text(&self, path: &str) -> Option<String> {
        if self.tree.files.contains(path) {
            return self.show(path);
        }
        let directory = format!("{}/", path.trim_end_matches('/'));
        let files: Vec<&String> = self.tree.files.range(directory.clone()..).take_while(|file| file.starts_with(&directory)).collect();
        (!files.is_empty()).then(|| files.into_iter().filter_map(|file| self.show(file)).collect())
    }
}

fn pointing_at_nothing(revision: &Revision, documents: &BTreeMap<String, String>) -> BTreeSet<(String, String)> {
    let tree = revision.tree;
    let mut found: BTreeSet<(String, String)> = documents
        .iter()
        .flat_map(|(name, text)| {
            let paths = missing_paths(text, tree).into_iter().map(|path| format!("names `{path}`, which does not exist"));
            let links = broken_links(name, text, tree).into_iter().map(|target| format!("links to {target}, which does not exist"));
            paths.chain(links).map(|reference| (name.clone(), reference)).collect::<Vec<_>>()
        })
        .collect();
    if let Some(doc) = documents.get(cites::FEEDBACK_LOOP) {
        found.extend(cites::dead_readers(doc, revision).into_iter().map(|fault| (cites::FEEDBACK_LOOP.to_owned(), format!("cites {fault}"))));
    }
    if let Some(registry) = revision.show(cites::LOOPS) {
        found.extend(cites::dead_loop_sites(&registry, revision).into_iter().map(|fault| (cites::LOOPS.to_owned(), format!("cites {fault}"))));
    }
    found
}

fn listing(directory: &Path, revision: &str) -> Option<Tree> {
    git::text(directory, &["ls-tree", "-r", "--name-only", revision]).map(|names| Tree::new(&names))
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "doc-links: expected BASE..HEAD, got {range}");
        return 2;
    };
    let fork = git::merge_base(directory, base, head);
    let (Some(after), Some(tree)) = (documents(directory, head), listing(directory, head)) else {
        let _ = writeln!(out, "doc-links: cannot read {head}");
        return 1;
    };
    let now = pointing_at_nothing(&Revision { directory, revision: head, tree: &tree }, &after);
    let before = match (documents(directory, &fork), listing(directory, &fork)) {
        (Some(documents), Some(tree)) if !now.is_empty() => pointing_at_nothing(&Revision { directory, revision: &fork, tree: &tree }, &documents),
        _ => BTreeSet::new(),
    };
    let mut refused = false;
    for (name, reference) in now.difference(&before) {
        refused = true;
        let _ = writeln!(out, "doc-links: {name} {reference}; fix or delete the line (a `Creates:` line lists files the work will add)");
    }
    if refused {
        let _ = writeln!(out, "doc-links: a lane may not leave README.md, AGENTS.md, docs/ or the loop registry pointing at nothing (north#635, north#722)");
    }
    u8::from(refused)
}

#[cfg(test)]
#[path = "doc_links_tests.rs"]
mod tests;
