use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::git;

static TEST_ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#\[(?:cfg\(test\)|(?:[[:alnum:]_]+::)*test\b)").expect("test item pattern"));
static HUNK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@@ -[0-9]+(?:,[0-9]+)? \+([0-9]+)(?:,([0-9]+))? @@").expect("hunk pattern"));

pub(crate) fn touched(directory: &Path, base: &str, head: &str) -> bool {
    let diff = git::text(directory, &["diff", "--unified=0", "--no-color", "--no-ext-diff", "--no-renames", base, head]).unwrap_or_default();
    added_lines(&diff).into_iter().filter(|(path, _)| path.ends_with(".rs")).any(|(path, lines)| {
        let Some(source) = git::text(directory, &["show", &format!("{head}:{path}")]) else { return false };
        let regions = test_regions(&source);
        lines.iter().any(|line| regions.iter().any(|region| region.contains(line)))
    })
}

fn added_lines(diff: &str) -> BTreeMap<String, Vec<usize>> {
    let mut found: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut path: Option<&str> = None;
    for line in diff.lines() {
        if let Some(name) = line.strip_prefix("+++ ") {
            path = name.strip_prefix("b/");
        } else if let (Some(path), Some(hunk)) = (path, HUNK.captures(line)) {
            let start: usize = hunk[1].parse().unwrap_or(0);
            let count: usize = hunk.get(2).map_or(1, |count| count.as_str().parse().unwrap_or(0));
            found.entry(path.to_owned()).or_default().extend(start..start + count);
        }
    }
    found
}

fn test_regions(source: &str) -> Vec<RangeInclusive<usize>> {
    let masked = mask(source);
    let lines: Vec<&str> = masked.lines().collect();
    (0..lines.len()).filter(|&start| TEST_ITEM.is_match(lines[start].trim_start())).map(|start| start + 1..=item_end(&lines, start) + 1).collect()
}

#[derive(Default)]
struct Nesting {
    brackets: i32,
    braces: i32,
    opened: bool,
}

impl Nesting {
    fn closes(&mut self, character: char) -> bool {
        match character {
            '(' | '[' => self.brackets += 1,
            ')' | ']' => self.brackets -= 1,
            ';' if !self.opened && self.brackets <= 0 => return true,
            '{' if self.brackets <= 0 => {
                self.braces += 1;
                self.opened = true;
            }
            '}' if self.brackets <= 0 && self.opened => {
                self.braces -= 1;
                return self.braces == 0;
            }
            _ => {}
        }
        false
    }
}

fn item_end(lines: &[&str], start: usize) -> usize {
    let mut nesting = Nesting::default();
    (start..lines.len()).find(|&index| lines[index].chars().any(|character| nesting.closes(character))).unwrap_or(lines.len().saturating_sub(1))
}

fn blank(character: char) -> char {
    if character == '\n' { '\n' } else { ' ' }
}

fn raw_string_end(text: &[char], at: usize) -> Option<usize> {
    let hashes = text[at + 1..].iter().take_while(|&&character| character == '#').count();
    (text.get(at + 1 + hashes) == Some(&'"')).then(|| {
        let closing: Vec<char> = std::iter::once('"').chain(std::iter::repeat_n('#', hashes)).collect();
        (at + 2 + hashes..text.len()).find(|&index| text[index..].starts_with(&closing)).map_or(text.len(), |index| index + closing.len())
    })
}

fn string_end(text: &[char], mut at: usize) -> usize {
    while at < text.len() {
        match text[at] {
            '\\' => at += 2,
            '"' => return at + 1,
            _ => at += 1,
        }
    }
    text.len()
}

fn block_comment_end(text: &[char], mut at: usize) -> usize {
    let mut depth = 0;
    while at < text.len() {
        if text[at..].starts_with(&['/', '*']) {
            depth += 1;
            at += 2;
        } else if text[at..].starts_with(&['*', '/']) {
            depth -= 1;
            at += 2;
            if depth == 0 {
                return at;
            }
        } else {
            at += 1;
        }
    }
    text.len()
}

fn after_word(text: &[char], at: usize) -> bool {
    at > 0 && (text[at - 1].is_alphanumeric() || text[at - 1] == '_') && text[at - 1] != 'b'
}

fn comment_end(text: &[char], at: usize) -> Option<usize> {
    match text.get(at + 1) {
        Some('/') => Some((at..text.len()).find(|&index| text[index] == '\n').unwrap_or(text.len())),
        Some('*') => Some(block_comment_end(text, at)),
        _ => None,
    }
}

fn character_end(text: &[char], at: usize) -> Option<usize> {
    match (text.get(at + 1), text.get(at + 2)) {
        (Some('\\'), _) => Some((at + 3..text.len()).find(|&index| text[index] == '\'').map_or(text.len(), |index| index + 1)),
        (Some(_), Some('\'')) => Some(at + 3),
        _ => None,
    }
}

fn literal_end(text: &[char], at: usize) -> Option<usize> {
    match text[at] {
        '/' => comment_end(text, at),
        '"' => Some(string_end(text, at + 1)),
        'r' if !after_word(text, at) => raw_string_end(text, at),
        '\'' => character_end(text, at),
        _ => None,
    }
}

fn mask(source: &str) -> String {
    let text: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    while at < text.len() {
        let end = literal_end(&text, at).unwrap_or(at + 1);
        out.extend(text[at..end].iter().map(|&character| if end == at + 1 { character } else { blank(character) }));
        at = end;
    }
    out
}
