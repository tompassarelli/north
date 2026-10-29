use super::{GREP_LIKE, find, grep};
use crate::guard::search_words::{self, Located};
use crate::shell::{self, Piece};
use std::collections::BTreeSet;

type Edit = (usize, usize, String);

pub(super) fn rewrite(command: &str, base: &str, home: &str) -> Option<String> {
    let chars: Vec<char> = command.chars().collect();
    let pieces = shell::pieces(&shell::strip_heredocs(command))?;
    let mut edits = Vec::new();
    for segment in shell::piece_segments(&pieces) {
        edits.extend(segment_edit(&chars, &segment, base, home)?);
    }
    (!edits.is_empty()).then(|| shell::splice(&chars, edits))
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\"'\"'"))
    }
}

fn expands(source: &str) -> bool {
    let mut quote = None;
    for c in source.chars() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(open), _) if open == c => quote = None,
            (Some('\''), _) => {}
            (Some(_), '$' | '`') => return true,
            (Some(_), _) => {}
            (None, '$' | '`' | '*' | '?' | '[' | '~' | '{' | '\\') => return true,
            (None, _) => {}
        }
    }
    false
}

fn redirect(source: &str) -> bool {
    source
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .starts_with(['>', '<'])
}

fn operator_only(source: &str) -> bool {
    source
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .chars()
        .all(|c| matches!(c, '>' | '<'))
}

fn segment_edit(chars: &[char], segment: &[Piece], base: &str, home: &str) -> Option<Option<Edit>> {
    let sources: Vec<String> = segment.iter().map(|piece| shell::slice(chars, *piece)).collect();
    let values = unquoted_words(&sources)?;
    let is_tool = |name: &str| GREP_LIKE.contains(&name) || name == "find";
    let index = match search_words::locate(&values, &["{", "}", "!"], is_tool) {
        Located::Tool(index) => index,
        Located::Nested(_) | Located::Other => return Some(None),
    };
    let (operands, redirects) = split_redirects(&sources, index + 1);
    let mut arguments: Vec<String> = operands.iter().map(|&i| values[i].clone()).collect();
    let executable = shell::basename(&values[index]);
    let Some(replacement) = (if executable == "find" {
        find::to_fd(&arguments)
    } else {
        if executable == "rgrep" {
            arguments.insert(0, "-r".into());
        }
        grep::translate(&arguments, base, home, true)
    }) else {
        return Some(None);
    };
    let mut words = vec![replacement[0].clone()];
    for word in &replacement[1..] {
        let found: BTreeSet<&str> = operands
            .iter()
            .filter(|&&i| values[i] == *word)
            .map(|&i| sources[i].as_str())
            .collect();
        match found.len() {
            0 => words.push(quote(word)),
            1 => words.push(found.into_iter().next()?.to_owned()),
            _ => return None,
        }
    }
    if operands
        .iter()
        .any(|&i| expands(&sources[i]) && !words.contains(&sources[i]) && !option_glob(&sources[i]))
    {
        return None;
    }
    words.extend(redirects.iter().map(|&i| sources[i].clone()));
    let last = segment.last()?;
    Some(Some((segment[index].start, last.end, words.join(" "))))
}

fn option_glob(source: &str) -> bool {
    source.starts_with('-') && !source.contains(['$', '`', '~', '{', '\\'])
}

fn unquoted_words(sources: &[String]) -> Option<Vec<String>> {
    sources
        .iter()
        .map(|source| match shell::shlex_split(source).as_deref() {
            Some([word]) => Some(word.clone()),
            _ => None,
        })
        .collect()
}

fn split_redirects(sources: &[String], from: usize) -> (Vec<usize>, Vec<usize>) {
    let (mut operands, mut redirects) = (Vec::new(), Vec::new());
    let mut next = from;
    while next < sources.len() {
        if redirect(&sources[next]) {
            redirects.push(next);
            if operator_only(&sources[next]) && next + 1 < sources.len() {
                next += 1;
                redirects.push(next);
            }
        } else {
            operands.push(next);
        }
        next += 1;
    }
    (operands, redirects)
}
