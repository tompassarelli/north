use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Absent,
    Content,
    Exec,
    Listing,
    Unreusable,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Content => "content",
            Self::Exec => "exec",
            Self::Listing => "listing",
            Self::Unreusable => "unreusable",
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        [Self::Absent, Self::Content, Self::Exec, Self::Listing, Self::Unreusable].into_iter().find(|kind| kind.word() == word)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entry {
    pub kind: Kind,
    pub path: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub reason: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "path set line {}: {}", self.line, self.reason)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(text: &str) -> Result<Vec<Entry>, ParseError> {
    let mut entries = text.lines().enumerate().map(|(index, line)| parse_line(index + 1, line)).collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    entries.dedup();
    Ok(entries)
}

pub fn render(entries: &[Entry]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort();
    sorted.dedup();
    sorted.iter().map(|entry| format!("{}\t{}\n", entry.kind.word(), entry.path)).collect()
}

fn parse_line(line: usize, text: &str) -> Result<Entry, ParseError> {
    let fail = |reason| ParseError { line, reason };
    let (word, path) = text.split_once('\t').ok_or_else(|| fail("expected KIND<TAB>PATH"))?;
    let kind = Kind::from_word(word).ok_or_else(|| fail("unknown kind"))?;
    if kind == Kind::Unreusable {
        return if path.is_empty() || path.contains(['\t', '\n']) { Err(fail("unreusable needs a one-line reason")) } else { Ok(Entry { kind, path: path.to_owned() }) };
    }
    if path.is_empty() || path.starts_with('/') || path.contains('\t') || path.split('/').any(|part| part == ".." || part.is_empty()) {
        return Err(fail("path must be relative to the tree, without empty or .. parts"));
    }
    Ok(Entry { kind, path: path.to_owned() })
}
