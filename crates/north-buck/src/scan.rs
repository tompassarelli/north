use std::collections::BTreeSet;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reads {
    pub includes: BTreeSet<String>,
    pub manifest_includes: BTreeSet<String>,
    pub module_paths: BTreeSet<String>,
    pub env: BTreeSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Ident(String),
    Str(String),
    Punct(char),
}

pub fn reads(source: &str) -> Reads {
    let tokens = tokenize(source);
    let mut found = Reads::default();
    for at in 0..tokens.len() {
        let rest = &tokens[at..];
        if let Some(path) = include(rest) {
            found.includes.insert(path.to_owned());
        } else if let Some(path) = manifest_include(rest) {
            found.manifest_includes.insert(path.to_owned());
        } else if let Some(var) = env_read(rest) {
            found.env.insert(var.to_owned());
        } else if let Some(path) = module_path(rest) {
            found.module_paths.insert(path.to_owned());
        }
    }
    found
}

fn include(tokens: &[Token]) -> Option<&str> {
    match tokens {
        [Token::Ident(name), Token::Punct('!'), Token::Punct('('), Token::Str(path), ..] if is_include(name) => Some(path),
        _ => None,
    }
}

fn manifest_include(tokens: &[Token]) -> Option<&str> {
    let [Token::Ident(name), Token::Punct('!'), Token::Punct('('), Token::Ident(concat), Token::Punct('!'), Token::Punct('('), rest @ ..] = tokens else {
        return None;
    };
    let [Token::Ident(env), Token::Punct('!'), Token::Punct('('), Token::Str(var), Token::Punct(')'), Token::Punct(','), Token::Str(path), ..] = rest else {
        return None;
    };
    (is_include(name) && concat == "concat" && env == "env" && var == "CARGO_MANIFEST_DIR").then_some(path.as_str())
}

fn env_read(tokens: &[Token]) -> Option<&str> {
    match tokens {
        [Token::Ident(name), Token::Punct('!'), Token::Punct('('), Token::Str(var), ..] if name == "env" || name == "option_env" => Some(var),
        _ => None,
    }
}

fn module_path(tokens: &[Token]) -> Option<&str> {
    match tokens {
        [Token::Punct('#'), Token::Punct('['), Token::Ident(attribute), Token::Punct('='), Token::Str(path), Token::Punct(']'), ..] if attribute == "path" => Some(path),
        _ => None,
    }
}

fn is_include(name: &str) -> bool {
    matches!(name, "include_str" | "include_bytes" | "include")
}

fn tokenize(source: &str) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        at = skip_trivia(&chars, at).unwrap_or_else(|| push_token(&chars, at, &mut tokens));
    }
    tokens
}

fn skip_trivia(chars: &[char], at: usize) -> Option<usize> {
    match (chars[at], chars.get(at + 1)) {
        (character, _) if character.is_whitespace() => Some(at + 1),
        ('/', Some('/')) => Some(skip_line(chars, at)),
        ('/', Some('*')) => Some(skip_block_comment(chars, at)),
        ('\'', _) => Some(skip_quote(chars, at)),
        _ => None,
    }
}

fn push_token(chars: &[char], at: usize, tokens: &mut Vec<Token>) -> usize {
    if let Some((open, hashes)) = raw_string_start(chars, at) {
        return push_raw_string(chars, open, hashes, tokens);
    }
    match (chars[at], chars.get(at + 1)) {
        ('"', _) => push_string(chars, at + 1, tokens),
        ('b' | 'c', Some('"')) => push_string(chars, at + 2, tokens),
        (character, _) if character.is_alphanumeric() || character == '_' => push_ident(chars, at, tokens),
        (character, _) => {
            tokens.push(Token::Punct(character));
            at + 1
        }
    }
}

fn skip_line(chars: &[char], from: usize) -> usize {
    chars[from..].iter().position(|&character| character == '\n').map_or(chars.len(), |offset| from + offset + 1)
}

fn skip_block_comment(chars: &[char], from: usize) -> usize {
    let mut depth = 0usize;
    let mut at = from;
    while at + 1 < chars.len() {
        match (chars[at], chars[at + 1]) {
            ('/', '*') => {
                depth += 1;
                at += 2;
            }
            ('*', '/') => {
                depth -= 1;
                at += 2;
                if depth == 0 {
                    return at;
                }
            }
            _ => at += 1,
        }
    }
    chars.len()
}

fn push_string(chars: &[char], from: usize, tokens: &mut Vec<Token>) -> usize {
    let mut text = String::new();
    let mut at = from;
    while at < chars.len() {
        match chars[at] {
            '\\' => {
                if let Some(&escaped) = chars.get(at + 1) {
                    text.push(escaped);
                }
                at += 2;
            }
            '"' => {
                tokens.push(Token::Str(text));
                return at + 1;
            }
            character => {
                text.push(character);
                at += 1;
            }
        }
    }
    chars.len()
}

fn raw_string_start(chars: &[char], from: usize) -> Option<(usize, usize)> {
    let mut at = from;
    if matches!(chars.get(at), Some('b' | 'c')) {
        at += 1;
    }
    if chars.get(at) != Some(&'r') {
        return None;
    }
    at += 1;
    let hashes = chars[at..].iter().take_while(|&&character| character == '#').count();
    (chars.get(at + hashes) == Some(&'"')).then_some((at + hashes + 1, hashes))
}

fn push_raw_string(chars: &[char], open: usize, hashes: usize, tokens: &mut Vec<Token>) -> usize {
    let mut at = open;
    while at < chars.len() {
        if chars[at] == '"' && chars[at + 1..].iter().take(hashes).filter(|&&character| character == '#').count() == hashes {
            tokens.push(Token::Str(chars[open..at].iter().collect()));
            return at + 1 + hashes;
        }
        at += 1;
    }
    chars.len()
}

fn skip_quote(chars: &[char], from: usize) -> usize {
    match (chars.get(from + 1), chars.get(from + 2)) {
        (Some('\\'), _) => chars[from + 2..].iter().position(|&character| character == '\'').map_or(chars.len(), |offset| from + offset + 3),
        (Some(_), Some('\'')) => from + 3,
        _ => from + 1,
    }
}

fn push_ident(chars: &[char], from: usize, tokens: &mut Vec<Token>) -> usize {
    let length = chars[from..].iter().take_while(|&&character| character.is_alphanumeric() || character == '_').count();
    tokens.push(Token::Ident(chars[from..from + length].iter().collect()));
    from + length
}
