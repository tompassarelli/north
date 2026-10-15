use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const SEPARATORS: &str = ";&|()\n";
const PREFIXES: &[&str] = &[
    "sudo", "doas", "command", "env", "exec", "builtin", "nohup", "setsid",
];

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or("")
}

fn whitespace(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

fn assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn rewrite_input(data: &Value, field: &str, value: &str, context: &str) -> Value {
    let mut input = data["tool_input"].clone();
    input[field] = json!(value);
    json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": input, "additionalContext": context}})
}

pub fn argv_spans(text: &str) -> Result<Vec<(&'static str, usize, usize)>, &'static str> {
    fn quoted(text: &[char], mut cursor: usize, quote: char) -> Result<usize, &'static str> {
        cursor += 1;
        while cursor < text.len() {
            if text[cursor] == quote {
                return Ok(cursor + 1);
            }
            if quote == '"' && "$`".contains(text[cursor]) {
                return Err("unsupported expansion");
            }
            cursor += if quote == '"' && text[cursor] == '\\' {
                2
            } else {
                1
            };
        }
        Err("unclosed quote")
    }
    fn grouped(text: &[char], mut cursor: usize) -> Result<usize, &'static str> {
        let mut nesting = 1;
        cursor += 1;
        while cursor < text.len() {
            let c = text[cursor];
            if "'\"".contains(c) {
                cursor = quoted(text, cursor, c)?;
                continue;
            }
            if c == '\\' {
                cursor += 2;
                continue;
            }
            if c == '(' {
                nesting += 1;
            }
            if c == ')' {
                nesting -= 1;
                if nesting == 0 {
                    return Ok(cursor + 1);
                }
            }
            cursor += 1;
        }
        Err("unclosed substitution")
    }
    let text: Vec<_> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < text.len() {
        if " \t\r".contains(text[index]) {
            index += 1;
            continue;
        }
        let start = index;
        if SEPARATORS.contains(text[index]) {
            index += 1;
            tokens.push(("sep", start, index));
            continue;
        }
        while index < text.len()
            && !" \t\r".contains(text[index])
            && !SEPARATORS.contains(text[index])
        {
            let c = text[index];
            if "'\"".contains(c) {
                index = quoted(&text, index, c)?;
            } else if c == '\\' {
                if index + 1 >= text.len() {
                    return Err("unclosed escape");
                }
                index += 2;
            } else if c == '<' && text.get(index + 1) == Some(&'(') {
                index = grouped(&text, index + 1)?;
            } else if "<>`$#".contains(c) {
                return Err("unsupported shell operator");
            } else {
                index += 1;
            }
        }
        tokens.push(("word", start, index));
    }
    Ok(tokens)
}

pub fn blank(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\n' { '\n' } else { ' ' })
        .collect()
}

fn heredoc(text: &str, start: usize) -> Option<(usize, String, bool)> {
    let re = Regex::new(r#"<<-?[\s\x1c-\x1f]*(['"]?)([\p{L}\p{N}_]+)"#).unwrap();
    for m in re.captures_iter(&text[start..]) {
        let end = start + m.get(0).unwrap().end();
        let quote = &m[1];
        if quote.is_empty() || text[end..].starts_with(quote) {
            return Some((end + quote.len(), m[2].to_owned(), !quote.is_empty()));
        }
    }
    None
}

fn terminator(text: &str, start: usize, delimiter: &str) -> Option<(usize, usize)> {
    let re = Regex::new(&format!(r"(?m)^[ \t]*{}[ \t]*$", regex::escape(delimiter))).unwrap();
    re.find_at(text, start).map(|m| (m.start(), m.end()))
}

pub fn strip_heredocs(text: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < text.len() {
        let Some((end, delimiter, _)) = heredoc(text, i) else {
            out.push_str(&text[i..]);
            break;
        };
        let Some(line_end) = text[end..].find('\n').map(|n| end + n + 1) else {
            out.push_str(&text[i..]);
            break;
        };
        out.push_str(&text[i..line_end]);
        if let Some((start, _)) = terminator(text, line_end, &delimiter) {
            out.push_str(&blank(&text[line_end..start]));
            i = start;
        } else {
            out.push_str(&blank(&text[line_end..]));
            break;
        }
    }
    out
}

pub fn strip_quotes(text: &str) -> String {
    let text: Vec<_> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == '\'' || c == '"' {
            let mut j = i + 1;
            while j < text.len() {
                if c == '"' && text[j] == '\\' && j + 1 < text.len() {
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
    strip_prefixes(&out)
}

pub fn words(cmd: &str) -> Option<Vec<String>> {
    let mut text = String::new();
    let mut i = 0;
    let mut search = 0;
    while let Some((end, mut delimiter, quoted)) = heredoc(cmd, search) {
        search = end;
        let Some(body) = cmd[end..].find('\n').map(|n| end + n + 1) else {
            continue;
        };
        let mut close = terminator(cmd, body, &delimiter);
        while close.is_none() && !quoted && delimiter.chars().count() > 1 {
            delimiter.pop();
            close = terminator(cmd, body, &delimiter);
        }
        let Some((_, close)) = close else {
            continue;
        };
        text.push_str(&cmd[i..body]);
        text.push_str(&delimiter);
        i = close;
        search = close;
    }
    text.push_str(&cmd[i..]);
    let chars: Vec<_> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut buf = String::new();
    let mut started = false;
    let mut punctuation = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '#' {
            if started {
                tokens.push(std::mem::take(&mut buf));
                started = false;
            }
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            punctuation = false;
            continue;
        }
        if " \t\r".contains(c) {
            if started {
                tokens.push(std::mem::take(&mut buf));
                started = false;
            }
            punctuation = false;
            i += 1;
            continue;
        }
        let is_punct = SEPARATORS.contains(c);
        if started && is_punct != punctuation {
            tokens.push(std::mem::take(&mut buf));
        }
        punctuation = is_punct;
        if c == '\'' || c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != c {
                if c == '"' && chars[i] == '\\' {
                    i += 1;
                    if i == chars.len() {
                        return None;
                    }
                    if chars[i] != '\\' && chars[i] != '"' {
                        buf.push('\\');
                    }
                }
                buf.push(chars[i]);
                i += 1;
            }
            if i == chars.len() {
                return None;
            }
            i += 1;
            started = true;
        } else if c == '\\' {
            i += 1;
            if i == chars.len() {
                return None;
            }
            buf.push(chars[i]);
            i += 1;
            started = true;
        } else {
            buf.push(c);
            started = true;
            i += 1;
        }
    }
    if started {
        tokens.push(buf);
    }
    Some(tokens)
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

pub fn tokenize(
    text: &str,
    separators: &str,
    escaped_quoted: bool,
    groups: bool,
    whitespace: &str,
) -> Vec<(&'static str, String, bool)> {
    fn flush(
        tokens: &mut Vec<(&'static str, String, bool)>,
        buffer: &mut Vec<String>,
        quoted: &mut bool,
    ) {
        if !buffer.is_empty() {
            tokens.push(("word", buffer.concat(), *quoted));
        }
        buffer.clear();
        *quoted = false;
    }
    let text: Vec<_> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut buffer = Vec::<String>::new();
    let mut quoted = false;
    let mut index = 0;
    while index < text.len() {
        let c = text[index];
        if c == '\\' && index + 1 < text.len() {
            buffer.push(text[index + 1].to_string());
            quoted |= escaped_quoted;
            index += 2;
            continue;
        }
        if c == '\'' || c == '"' {
            let mut cursor = index + 1;
            let mut piece = String::new();
            while cursor < text.len() {
                if c == '"' && text[cursor] == '\\' && cursor + 1 < text.len() {
                    piece.push(text[cursor + 1]);
                    cursor += 2;
                    continue;
                }
                if text[cursor] == c {
                    break;
                }
                piece.push(text[cursor]);
                cursor += 1;
            }
            buffer.push(piece);
            quoted = true;
            index = cursor + 1;
            continue;
        }
        if separators.contains(c) {
            flush(&mut tokens, &mut buffer, &mut quoted);
            tokens.push(("sep", c.to_string(), false));
            index += 1;
            continue;
        }
        if groups
            && (c == ')'
                || (c == '(' && (buffer.is_empty() || buffer.last().is_some_and(|s| s == "$"))))
        {
            if c == '(' && buffer.last().is_some_and(|s| s == "$") {
                buffer.pop();
            }
            flush(&mut tokens, &mut buffer, &mut quoted);
            tokens.push(("sep", c.to_string(), false));
            index += 1;
            continue;
        }
        if whitespace.contains(c) {
            flush(&mut tokens, &mut buffer, &mut quoted);
            index += 1;
            continue;
        }
        buffer.push(c.to_string());
        index += 1;
    }
    flush(&mut tokens, &mut buffer, &mut quoted);
    tokens
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

#[cfg(test)]
#[path = "shellcmd_tests.rs"]
mod tests;
