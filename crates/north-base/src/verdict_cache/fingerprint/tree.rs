use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::verdict_cache::path_set::Kind;

const DIRECTORY_MODE: &str = "040000";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    Blob(String),
    Directory,
}

#[derive(Debug, Default)]
pub struct Tree {
    nodes: BTreeMap<String, Node>,
    listings: BTreeMap<String, Vec<(String, String)>>,
}

impl Tree {
    pub fn load(repo: &Path, revision: &str) -> io::Result<Self> {
        let output = Command::new("git").arg("-C").arg(repo).args(["ls-tree", "-r", "-t", "-z", "--full-tree", revision]).stdin(Stdio::null()).output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!("git ls-tree {revision}: {}", String::from_utf8_lossy(&output.stderr).trim_end())));
        }
        Ok(Self::parse(&output.stdout))
    }

    pub fn parse(listing: &[u8]) -> Self {
        let mut tree = Self::default();
        for record in String::from_utf8_lossy(listing).split('\0').filter(|record| !record.is_empty()) {
            let Some((meta, path)) = record.split_once('\t') else { continue };
            let mut words = meta.split(' ');
            let (Some(mode), Some(kind), Some(id)) = (words.next(), words.next(), words.next()) else { continue };
            let node = if kind == "tree" { Node::Directory } else { Node::Blob(format!("{mode} {id}")) };
            let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
            let mode = if kind == "tree" { DIRECTORY_MODE } else { mode };
            tree.listings.entry(parent.to_owned()).or_default().push((name.to_owned(), mode.to_owned()));
            tree.nodes.insert(path.to_owned(), node);
        }
        for names in tree.listings.values_mut() {
            names.sort();
        }
        tree
    }

    pub fn blob(&self, path: &str) -> Option<&str> {
        match self.nodes.get(path)? {
            Node::Blob(text) => Some(text),
            Node::Directory => None,
        }
    }

    fn exists(&self, path: &str) -> bool {
        path == "." || self.nodes.contains_key(path)
    }

    pub fn content(&self, path: &str) -> String {
        match self.nodes.get(path) {
            Some(Node::Blob(text)) => text.clone(),
            Some(Node::Directory) => "directory".to_owned(),
            None => "absent".to_owned(),
        }
    }

    fn listing(&self, path: &str) -> String {
        let key = if path == "." { "" } else { path };
        if key.is_empty() || self.nodes.get(key) == Some(&Node::Directory) {
            self.listings.get(key).map_or_else(String::new, |names| names.iter().map(|(name, mode)| format!("{mode} {name}\n")).collect())
        } else {
            "not a directory".to_owned()
        }
    }

    pub fn answer(&self, kind: Kind, path: &str) -> Option<String> {
        match kind {
            Kind::Absent => Some(if self.exists(path) { "present" } else { "absent" }.to_owned()),
            Kind::Content | Kind::Exec => Some(self.content(path)),
            Kind::Listing => Some(self.listing(path)),
            Kind::Unreusable => None,
        }
    }
}
