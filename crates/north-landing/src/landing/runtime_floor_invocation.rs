use std::sync::LazyLock;

use regex::Regex;

static HEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:^|[\s;&|({`!"'])north\s+|\}/north\s+|/bin/north\s+"#).expect("head pattern"));
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9-]*$").expect("word pattern"));
static FLAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^["'`(]*(--[a-z][a-z0-9-]*)"#).expect("flag pattern"));

const ENDS: [&str; 6] = [";", "&", "|", ">", "<", "`"];
const WORD_LIMIT: usize = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invocation {
    pub words: Vec<String>,
    pub flags: Vec<String>,
}

impl Invocation {
    pub fn command(&self) -> Option<&str> {
        self.words.first().map(String::as_str)
    }

    pub fn path(&self) -> String {
        self.words.join(" ")
    }
}

pub fn invocations(line: &str) -> Vec<Invocation> {
    let code = line.trim_start();
    if code.starts_with('#') || code.starts_with("//") {
        return Vec::new();
    }
    let heads: Vec<_> = HEAD.find_iter(code).collect();
    heads
        .iter()
        .enumerate()
        .filter_map(|(at, head)| {
            let end = heads.get(at + 1).map_or(code.len(), |next| next.start());
            read(&code[head.end()..end])
        })
        .collect()
}

fn closes(token: &str) -> bool {
    ENDS.iter().any(|end| token.contains(end)) || token.matches(')').count() > token.matches('(').count()
}

fn read(tail: &str) -> Option<Invocation> {
    let mut words = Vec::new();
    let mut flags = Vec::new();
    let mut tokens = tail.split_whitespace().peekable();
    while let Some(token) = tokens.peek().copied() {
        let bare = token.trim_end_matches(['"', '\'', ';', ')', '`']);
        if words.len() >= WORD_LIMIT || !WORD.is_match(bare) {
            break;
        }
        words.push(bare.to_owned());
        tokens.next();
        if bare.len() != token.len() {
            return Some(Invocation { words, flags });
        }
    }
    for token in tokens {
        if let Some(flag) = FLAG.captures(token) {
            flags.push(flag[1].to_owned());
        }
        if closes(token) {
            break;
        }
    }
    (!words.is_empty()).then_some(Invocation { words, flags })
}
