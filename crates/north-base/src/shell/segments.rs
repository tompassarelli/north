use regex::Regex;
use std::collections::BTreeMap;
use super::lexer::{SEPARATORS, assignment, basename, whitespace};

const PREFIXES: &[&str] = &[
    "sudo", "doas", "command", "env", "exec", "builtin", "nohup", "setsid",
];
pub fn strip_quotes(text: &str) -> String {
    strip_prefixes(&blank_quotes(text))
}

pub fn blank_quotes(text: &str) -> String {
    let text: Vec<_> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        let ansi_c = c == '$' && text.get(i + 1) == Some(&'\'');
        if ansi_c || c == '\'' || c == '"' {
            let c = if ansi_c { '\'' } else { c };
            let mut j = i + if ansi_c { 2 } else { 1 };
            while j < text.len() {
                if (c == '"' || ansi_c) && text[j] == '\\' && j + 1 < text.len() {
                    j += 2;
                    continue;
                }
                if text[j] == c {
                    j += 1;
                    break;
                }
                j += 1;
            }
            out.extend(
                text[i..j]
                    .iter()
                    .map(|c| if *c == '\n' { '\n' } else { ' ' }),
            );
            i = j;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}
pub fn is_separator(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|c| SEPARATORS.contains(c))
}

pub fn segments(tokens: &[String]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut segment = Vec::new();
    for token in tokens {
        if is_separator(token) {
            out.push(std::mem::take(&mut segment));
        } else {
            segment.push(token.clone());
        }
    }
    out.push(segment);
    out
}

pub fn command(segment: &[String], prefixes: &[&str]) -> usize {
    let mut wrapper = false;
    let mut index = 0;
    while index < segment.len() {
        let word = segment[index].as_str();
        let name = basename(word);
        if wrapper && ["-u", "--user", "-g", "--group", "-C", "--chdir", "--unset"].contains(&word)
        {
            index += 2;
            continue;
        }
        if assignment(word) || word == "--" || (word.starts_with('-') && index > 0) {
            index += 1;
            continue;
        }
        if prefixes.contains(&name) || ["command", "sudo", "doas"].contains(&name) {
            wrapper = true;
            index += 1;
            if name == "timeout" && index < segment.len() {
                let value = segment[index].strip_suffix('\n').unwrap_or(&segment[index]);
                let duration = value.trim_end_matches(['s', 'm', 'h', 'd']);
                if !duration.is_empty()
                    && duration.chars().all(|c| c.is_ascii_digit() || c == '.')
                    && value.len() - duration.len() <= 1
                {
                    index += 1;
                }
            }
            continue;
        }
        break;
    }
    index
}

pub fn strip_prefixes(text: &str) -> String {
    let re = Regex::new(r"(?:[\n;&|({`]|^)((?:\$\{[^$(){};&|`\n]*\}|[^\n;&|(){}`])*)").unwrap();
    let token_re = Regex::new(r"[^\s\x1c-\x1f]+").unwrap();
    let mut out: Vec<_> = text.chars().collect();
    for captures in re.captures_iter(text) {
        let span = captures.get(1).unwrap();
        let tokens: Vec<_> = token_re.find_iter(span.as_str()).collect();
        let index = command(
            &tokens
                .iter()
                .map(|m| m.as_str().to_owned())
                .collect::<Vec<_>>(),
            PREFIXES,
        );
        if index > 0 {
            let end = tokens[index.min(tokens.len()) - 1].end();
            let start = text[..span.start()].chars().count();
            for c in &mut out[start..start + span.as_str()[..end].chars().count()] {
                if *c != '\n' {
                    *c = ' ';
                }
            }
        }
    }
    out.into_iter().collect()
}
pub fn shell_segments_sep(text: &str) -> Vec<(String, Option<String>)> {
    let text: Vec<_> = text.chars().collect();
    let mut segments = Vec::new();
    let mut buf = String::new();
    let mut quote = None;
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if let Some(q) = quote {
            buf.push(c);
            if q == '"' && c == '\\' && i + 1 < text.len() {
                i += 1;
                buf.push(text[i]);
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            buf.push(c);
            i += 1;
            continue;
        }
        if c == '\\' && i + 1 < text.len() {
            buf.push(c);
            i += 1;
            buf.push(text[i]);
            i += 1;
            continue;
        }
        if ";|&\n".contains(c) {
            let mut sep = if c == '\n' {
                ";".to_owned()
            } else {
                c.to_string()
            };
            if i + 1 < text.len()
                && ((c == '&' && text[i + 1] == '&') || (c == '|' && text[i + 1] == '|'))
            {
                sep.push(c);
                i += 1;
            }
            if !buf.trim_matches(whitespace).is_empty() {
                segments.push((buf.trim_matches(whitespace).to_owned(), Some(sep)));
            }
            buf.clear();
            i += 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    if !buf.trim_matches(whitespace).is_empty() {
        segments.push((buf.trim_matches(whitespace).to_owned(), None));
    }
    segments
}

pub fn command_index(tokens: &[String]) -> usize {
    let mut i = 0;
    while i < tokens.len() {
        let word = basename(&tokens[i]).trim_start_matches('(');
        if assignment(&tokens[i])
            || ["!", "{", "if", "then", "elif", "while", "until", "do"].contains(&word)
        {
            i += 1;
            continue;
        }
        if !["env", "command", "builtin", "exec", "nohup", "sudo", "time"].contains(&word) {
            break;
        }
        i += 1;
        let consumes: &[&str] = match word {
            "env" => &["-u", "--unset", "-C", "--chdir", "-S", "--split-string"],
            "sudo" => &[
                "-u", "--user", "-g", "--group", "-h", "--host", "-p", "--prompt", "-C", "--chdir",
                "-r", "--role", "-t", "--type",
            ],
            _ => &[],
        };
        while i < tokens.len() {
            let arg = tokens[i].as_str();
            if assignment(arg) {
                i += 1;
            } else if consumes.contains(&arg) && i + 1 < tokens.len() {
                i += 2;
            } else if arg == "--" {
                i += 1;
                break;
            } else if arg.starts_with('-') {
                i += 1;
            } else {
                break;
            }
        }
    }
    i
}

pub fn tripwire_word(token: &str) -> &str {
    let mut word = token.strip_prefix('(').unwrap_or(token);
    word = word.strip_prefix('{').unwrap_or(word);
    for c in ['"', '\''] {
        word = word.strip_prefix(c).unwrap_or(word);
        word = word.strip_suffix(c).unwrap_or(word);
    }
    word = basename(word.strip_suffix(')').unwrap_or(word));
    if word == "-u" {
        return "_skip_user";
    }
    if [
        "sudo", "doas", "command", "builtin", "nohup", "nice", "ionice", "stdbuf", "eval", "exec",
        "time", "timeout", "env", "", "{", "}", "(", ")", "if", "then", "elif", "else", "fi", "do",
        "done", "while", "until", "for", "!",
    ]
    .contains(&word)
        || word.starts_with('-')
        || word.starts_with(|c: char| c.is_ascii_digit())
        || (word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && word.split('\n').next().unwrap_or("").contains('='))
    {
        return "";
    }
    word
}

pub fn tripwire_tokens(text: &str) -> Vec<String> {
    let mut marker = "\x04".to_owned();
    while text.contains(&marker) {
        marker.push('\x04');
    }
    let mut quoted = BTreeMap::new();
    let mut pieces = String::new();
    let chars: Vec<_> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if c == '\\' && index + 1 < chars.len() {
            pieces.extend(&chars[index..index + 2]);
            index += 2;
            continue;
        }
        if c == '\'' || c == '"' {
            let mut end = index + 1;
            while end < chars.len() {
                if c == '"' && chars[end] == '\\' && end + 1 < chars.len() {
                    end += 2;
                } else if chars[end] == c {
                    end += 1;
                    break;
                } else {
                    end += 1;
                }
            }
            let value: String = chars[index..end].iter().collect();
            if c == '\'' || (!value.contains("$(") && !value.contains('`')) {
                let key = format!("{marker}{}{marker}", quoted.len());
                quoted.insert(key.clone(), value);
                pieces.push_str(&key);
            } else {
                pieces.push_str(&value);
            }
            index = end;
            continue;
        }
        pieces.push(c);
        index += 1;
    }
    let mut text = pieces
        .replace("\\\n", " ")
        .replace('\n', " ; ")
        .replace('\t', " ")
        .replace("$(", "\x02 ; ( ");
    let mut opening = true;
    while text.contains('`') {
        text = text.replacen('`', if opening { "\x02 ; ( " } else { " ) ; " }, 1);
        opening = !opening;
    }
    text = text
        .replace("&&", " \x01 ")
        .replace("||", " \x03 ")
        .replace(';', " ; ")
        .replace('|', " | ")
        .replace('&', " ; ")
        .replace('\x01', "&&")
        .replace('\x03', "||");
    text.split([' ', '\t', '\n'])
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut word = word.to_owned();
            for (key, value) in &quoted {
                word = word.replace(key, value);
            }
            word
        })
        .collect()
}
