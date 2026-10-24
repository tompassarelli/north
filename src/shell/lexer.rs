use super::heredoc::{heredoc, terminator};
use super::scan::strip_comments;

pub const SEPARATORS: &str = ";&|()\n";
pub fn shlex_split(text: &str) -> Option<Vec<String>> {
    let mut chars = text.chars();
    let mut tokens = Vec::new();
    let mut buffer = String::new();
    let mut quote = None;
    let mut started = false;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => buffer.push(c),
            (Some('"'), '\\') => {
                let next = chars.next()?;
                if next != '"' && next != '\\' {
                    buffer.push('\\');
                }
                buffer.push(next);
            }
            (Some(_), _) => buffer.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                buffer.push(chars.next()?);
                started = true;
            }
            (None, ' ' | '\t' | '\r' | '\n') => {
                if started {
                    tokens.push(std::mem::take(&mut buffer));
                }
                started = false;
            }
            (None, _) => {
                buffer.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        tokens.push(buffer);
    }
    Some(tokens)
}
pub fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or("")
}

pub fn whitespace(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

pub fn assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
#[allow(dead_code)] // exercised by tests and the shellcmd_compare example, not the binary
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
    lex(&strip_comments(&text))
}

pub fn lex(text: &str) -> Option<Vec<String>> {
    let chars: Vec<_> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut buf = String::new();
    let mut started = false;
    let mut punctuation = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
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

#[derive(Debug)]
pub enum ShellToken {
    Word(String),
    Operator(String),
}

pub fn operator_tokens(command: &str) -> Result<Vec<ShellToken>, &'static str> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && quote != Some('\'') {
            word.push(chars.next().ok_or("unfinished escape")?);
        } else if Some(ch) == quote {
            quote = None;
        } else if quote.is_some() {
            word.push(ch);
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
        } else if ch.is_whitespace() || ";|&><()".contains(ch) {
            if !word.is_empty() {
                tokens.push(ShellToken::Word(std::mem::take(&mut word)));
            }
            if !ch.is_whitespace() || ch == '\n' {
                let mut operator = ch.to_string();
                if chars.peek() == Some(&ch) && ">|&<".contains(ch) {
                    operator.push(chars.next().unwrap());
                }
                tokens.push(ShellToken::Operator(operator));
            }
        } else {
            word.push(ch);
        }
    }
    if quote.is_some() {
        return Err("unfinished quote");
    }
    if !word.is_empty() {
        tokens.push(ShellToken::Word(word));
    }
    Ok(tokens)
}
