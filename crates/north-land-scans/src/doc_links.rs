use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

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

fn pointing_at_nothing(documents: &BTreeMap<String, String>, tree: &Tree) -> BTreeSet<(String, String)> {
    documents
        .iter()
        .flat_map(|(name, text)| {
            let paths = missing_paths(text, tree).into_iter().map(|path| format!("names `{path}`"));
            let links = broken_links(name, text, tree).into_iter().map(|target| format!("links to {target}"));
            paths.chain(links).map(|reference| (name.clone(), reference)).collect::<Vec<_>>()
        })
        .collect()
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
    let now = pointing_at_nothing(&after, &tree);
    let before = match (documents(directory, &fork), listing(directory, &fork)) {
        (Some(documents), Some(tree)) if !now.is_empty() => pointing_at_nothing(&documents, &tree),
        _ => BTreeSet::new(),
    };
    let mut refused = false;
    for (name, reference) in now.difference(&before) {
        refused = true;
        let _ = writeln!(out, "doc-links: {name} {reference}, which does not exist; fix or delete the line (a `Creates:` line lists files the work will add)");
    }
    if refused {
        let _ = writeln!(out, "doc-links: a lane may not leave README.md, AGENTS.md or docs/ pointing at nothing (north#635)");
    }
    u8::from(refused)
}
