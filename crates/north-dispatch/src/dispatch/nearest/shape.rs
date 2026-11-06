//! A box's shape: what its text names (repository, files, the code areas they sit in, symbols) and the
//! words it uses, read from the text alone so that no self-declared label decides it.
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

/// Header lines whose value is a routing decision or a self-declared label, never a description of the work.
const LABELS: [&str; 12] = ["Tier", "Arm", "ETA", "Budget", "Category", "Spec", "Scope", "Surface", "Verify", "Tier-reason", "Delegation", "Follows"];
const STOP_WORDS: [&str; 24] = ["the", "and", "for", "with", "that", "this", "from", "its", "are", "not", "into", "each", "when", "one", "two", "any", "per", "was", "has", "but", "then", "than", "only", "box"];

static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\W*Item:\s*(?:[\w-]+/)?([a-z][a-z0-9-]*)#\d+").expect("item pattern"));
static FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s`'\x22(\[,])((?:[\w.-]+/)+[\w.-]*\w\.[A-Za-z][A-Za-z0-9]{0,4})\b").expect("file pattern"));
static TICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)(?:\(\))?`").expect("symbol pattern"));
static LISTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.[A-Za-z]{1,5} \(([A-Za-z_][\w:, ]*)\)").expect("listed symbols pattern"));
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z][a-z0-9_-]{2,}").expect("word pattern"));

/// How far a box's named files reach: the blast radius a brief that names them is localized to.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Reach {
    Unnamed,
    OneFile,
    OneArea,
    Areas,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub repo: Option<String>,
    pub files: BTreeSet<String>,
    pub areas: BTreeSet<String>,
    pub symbols: BTreeSet<String>,
    pub words: BTreeMap<String, usize>,
}

impl Shape {
    /// The shape `text` describes, with `repo` standing in when no `Item:` line names one.
    pub fn read(text: &str, repo: Option<&str>) -> Self {
        let described: String = text.lines().filter(|line| !labelled(line)).collect::<Vec<_>>().join("\n");
        let files: BTreeSet<String> = FILE.captures_iter(&described).map(|found| relative(&found[1])).collect();
        let mut symbols: BTreeSet<String> = TICKED.captures_iter(&described).map(|found| found[1].to_owned()).filter(|word| symbolic(word)).collect();
        symbols.extend(LISTED.captures_iter(&described).flat_map(|found| found[1].split(',').map(|word| word.trim().to_owned()).collect::<Vec<_>>()).filter(|word| !word.is_empty()));
        let mut words = BTreeMap::new();
        for word in WORD.find_iter(&described.to_lowercase()) {
            let word = stem(word.as_str());
            if !STOP_WORDS.contains(&word) {
                *words.entry(word.to_owned()).or_insert(0) += 1;
            }
        }
        let repo = ITEM.captures(text).map(|found| found[1].to_owned()).or_else(|| repo.map(str::to_owned));
        let areas = files.iter().map(|file| area(file)).collect();
        Self { repo, files, areas, symbols, words }
    }

    /// The shape with the files its landings touched added: a finished box's realized footprint.
    pub fn touched(mut self, files: &BTreeSet<String>) -> Self {
        self.areas.extend(files.iter().map(|file| area(file)));
        self.files.extend(files.iter().cloned());
        self
    }

    /// A box that names both its files and its symbols is localized by construction.
    pub fn localized(&self) -> bool {
        !self.files.is_empty() && !self.symbols.is_empty()
    }

    pub fn reach(&self) -> Reach {
        match (self.files.len(), self.areas.len()) {
            (0, _) => Reach::Unnamed,
            (1, _) => Reach::OneFile,
            (_, 1) => Reach::OneArea,
            _ => Reach::Areas,
        }
    }
}

fn labelled(line: &str) -> bool {
    line.trim_start().split_once(':').is_some_and(|(key, _)| LABELS.iter().any(|label| key.trim().eq_ignore_ascii_case(label)))
}

fn symbolic(word: &str) -> bool {
    word.contains("::") || word.contains('_') || word.chars().skip(1).any(char::is_uppercase)
}

fn relative(path: &str) -> String {
    let path = path.trim_start_matches("./");
    ["crates/", "src/", "docs/", "harness/", "modules/"].iter().filter_map(|root| path.find(root).map(|at| &path[at..])).min_by_key(|rest| std::cmp::Reverse(rest.len())).unwrap_or(path).to_owned()
}

/// The code area a file sits in: its crate for `crates/NAME/...`, else its first directory.
fn area(file: &str) -> String {
    let mut parts = file.split('/');
    match (parts.next(), parts.next()) {
        (Some("crates"), Some(name)) => format!("crates/{name}"),
        (Some(first), Some(_)) => first.to_owned(),
        _ => ".".to_owned(),
    }
}

fn stem(word: &str) -> &str {
    let word = word.trim_end_matches(['-', '_']);
    ["ing", "ed", "es", "s"].iter().find_map(|suffix| word.strip_suffix(suffix).filter(|rest| rest.len() >= 4)).unwrap_or(word)
}
