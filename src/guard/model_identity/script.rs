use super::Context;
use super::identity::{calibration_command_placeholder, command_invalid, line_invalid, splitlines, text_invalid, trim};
use crate::guard::paths::basename;
use regex::Regex;
use std::sync::LazyLock;

const OPERATORS: [&str; 17] = [
    "&>>", "&>", "&&", "||", "|&", ">>", ">|", "<<", "<>", ">", "<", ";", "&", "|", "(", ")", "\n",
];
const CONTROLS: [&str; 7] = [";", "&&", "||", "&", "\n", "(", ")"];
const PIPELINES: [&str; 2] = ["|", "|&"];
const REDIRECTIONS: [&str; 8] = [">", ">>", ">|", "&>", "&>>", "<", "<<", "<>"];
const WRITES: [&str; 5] = [">", ">>", ">|", "&>", "&>>"];
const MAX_DEPTH: usize = 4;

static ASSIGNMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^[A-Za-z_][A-Za-z0-9_]*=.*\z").unwrap());

#[derive(Clone)]
struct Token {
    operator: bool,
    value: String,
    stat: bool,
}

struct Stage {
    words: Vec<(String, bool)>,
    writes: Vec<String>,
}

fn separator(set: &[&str], token: &Token) -> bool {
    token.operator && set.contains(&token.value.as_str())
}

struct Lexer {
    chars: Vec<char>,
    tokens: Vec<Token>,
    buffer: String,
    started: bool,
    stat: bool,
}

impl Lexer {
    fn flush(&mut self) {
        if self.started {
            self.tokens.push(Token { operator: false, value: std::mem::take(&mut self.buffer), stat: self.stat });
        }
        self.buffer.clear();
        self.started = false;
        self.stat = true;
    }

    fn operator(&self, at: usize) -> Option<&'static str> {
        OPERATORS
            .iter()
            .copied()
            .find(|candidate| candidate.chars().enumerate().all(|(k, c)| self.chars.get(at + k) == Some(&c)))
    }

    fn double_quoted(&mut self, mut at: usize) -> Option<usize> {
        self.started = true;
        at += 1;
        while at < self.chars.len() && self.chars[at] != '"' {
            if self.chars[at] == '\\' {
                self.buffer.push(*self.chars.get(at + 1)?);
                at += 2;
                continue;
            }
            if matches!(self.chars[at], '$' | '`') {
                self.stat = false;
            }
            self.buffer.push(self.chars[at]);
            at += 1;
        }
        (at < self.chars.len()).then_some(at + 1)
    }

    fn step(&mut self, at: usize) -> Option<usize> {
        let c = self.chars[at];
        if matches!(c, ' ' | '\t' | '\r') {
            self.flush();
            return Some(at + 1);
        }
        if c == '#' && !self.started {
            self.flush();
            let newline = self.chars[at..].iter().position(|&x| x == '\n');
            return match newline {
                Some(offset) => {
                    self.tokens.push(Token { operator: true, value: "\n".into(), stat: true });
                    Some(at + offset + 1)
                }
                None => Some(self.chars.len()),
            };
        }
        if let Some(operator) = self.operator(at) {
            self.flush();
            self.tokens.push(Token { operator: true, value: operator.into(), stat: true });
            return Some(at + operator.chars().count());
        }
        match c {
            '\\' => {
                self.buffer.push(*self.chars.get(at + 1)?);
                self.started = true;
                Some(at + 2)
            }
            '\'' => {
                let end = self.chars[at + 1..].iter().position(|&x| x == '\'')? + at + 1;
                self.started = true;
                self.buffer.extend(&self.chars[at + 1..end]);
                Some(end + 1)
            }
            '"' => self.double_quoted(at),
            '`' => None,
            '$' if self.chars.get(at + 1) == Some(&'(') => None,
            _ => {
                if c == '$' {
                    self.stat = false;
                }
                self.started = true;
                self.buffer.push(c);
                Some(at + 1)
            }
        }
    }
}

fn tokens(source: &str) -> Option<Vec<Token>> {
    let mut lexer = Lexer {
        chars: source.chars().collect(),
        tokens: Vec::new(),
        buffer: String::new(),
        started: false,
        stat: true,
    };
    let mut at = 0;
    while at < lexer.chars.len() {
        at = lexer.step(at)?;
    }
    lexer.flush();
    Some(lexer.tokens)
}

fn split<'a>(tokens: &[&'a Token], set: &[&str]) -> Vec<Vec<&'a Token>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    for &token in tokens {
        if separator(set, token) {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
        } else {
            current.push(token);
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn parse_stage(stage: &[&Token]) -> Option<Stage> {
    let mut words = Vec::new();
    let mut writes = Vec::new();
    let mut index = 0;
    while index < stage.len() {
        let token = stage[index];
        if token.operator {
            if !REDIRECTIONS.contains(&token.value.as_str()) {
                return None;
            }
            let target = stage.get(index + 1).filter(|t| !t.operator)?;
            if WRITES.contains(&token.value.as_str()) {
                writes.push(target.value.clone());
            }
            index += 2;
            continue;
        }
        words.push((token.value.clone(), token.stat));
        index += 1;
    }
    let leading = words.iter().take_while(|(w, _)| ASSIGNMENT.is_match(w)).count();
    words.drain(..leading);
    Some(Stage { words, writes })
}

fn static_scripts(name: &str, arguments: &[(String, bool)]) -> Vec<String> {
    if !matches!(name, "bash" | "sh") {
        return Vec::new();
    }
    let mut index = 0;
    while index < arguments.len() {
        let value = arguments[index].0.as_str();
        if value == "--" || !value.starts_with('-') || value == "-" {
            return Vec::new();
        }
        if value[1..].contains('c') {
            return match arguments.get(index + 1) {
                Some((script, true)) => vec![script.clone()],
                _ => Vec::new(),
            };
        }
        index += if matches!(value, "-o" | "-O") { 2 } else { 1 };
    }
    Vec::new()
}

fn sed_replacements(script: &str) -> Vec<String> {
    let stripped = trim(script);
    let chars: Vec<char> = stripped.chars().collect();
    if chars.len() < 4 || chars[0] != 's' {
        return Vec::new();
    }
    let delimiter = chars[1];
    if delimiter.is_alphanumeric() || delimiter.is_whitespace() || delimiter == '\\' {
        return Vec::new();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut buffer = String::new();
    let mut escaped = false;
    for &c in &chars[2..] {
        if escaped {
            buffer.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == delimiter {
            parts.push(std::mem::take(&mut buffer));
            if parts.len() == 2 {
                return vec![parts.remove(1)];
            }
        } else {
            buffer.push(c);
        }
    }
    Vec::new()
}

fn for_target_invalid(context: &Context, command: &str, target: &str) -> bool {
    if command_invalid(command) {
        return true;
    }
    let Some(path) = context.resolve(target) else {
        return false;
    };
    let name = basename(&path);
    (name == super::identity::CALIBRATION
        && (text_invalid(command, name) || calibration_command_placeholder(command)))
        || (name == super::identity::LEDGER
            && splitlines(command).iter().any(|line| line_invalid(line, super::identity::LEDGER)))
}

fn group_invalid(context: &Context, group: &[&Token], target: &str) -> bool {
    let words: Vec<&str> = group.iter().filter(|t| !t.operator).map(|t| t.value.as_str()).collect();
    if for_target_invalid(context, &words.join(" "), target) {
        return true;
    }
    context
        .resolve(target)
        .is_some_and(|path| words.iter().any(|word| text_invalid(word, basename(&path))))
}

fn in_place(name: &str, arguments: &[&str]) -> bool {
    arguments.iter().any(|a| {
        *a == "-i"
            || a.starts_with("-i")
            || *a == "--in-place"
            || a.starts_with("--in-place=")
            || (name == "perl" && a.starts_with("-p") && a.contains('i'))
    })
}

fn rewrites_invalid(context: &Context, name: &str, arguments: &[&str]) -> bool {
    if !in_place(name, arguments) {
        return false;
    }
    let written: Vec<String> = if name == "sed" {
        arguments.iter().flat_map(|a| sed_replacements(a)).collect()
    } else {
        vec![arguments.join(" ")]
    };
    arguments.iter().any(|candidate| {
        context.is_todo_record(candidate)
            && written.iter().any(|text| for_target_invalid(context, text, candidate))
    })
}

fn stage_invalid(context: &Context, command: &str, group: &[&Token], stage: &[&Token], parsed: &Stage, depth: usize) -> bool {
    if parsed.writes.iter().any(|t| context.is_todo_record(t) && group_invalid(context, stage, t)) {
        return true;
    }
    let Some((first, _)) = parsed.words.first() else {
        return false;
    };
    let name = basename(first);
    let arguments = &parsed.words[1..];
    if static_scripts(name, arguments).iter().any(|script| writes_invalid(context, script, depth + 1)) {
        return true;
    }
    if name == "apply_patch" && super::patch_invalid(context, command) {
        return true;
    }
    let values: Vec<&str> = arguments.iter().map(|(v, _)| v.as_str()).collect();
    if name == "tee"
        && values
            .iter()
            .any(|c| !c.starts_with('-') && context.is_todo_record(c) && group_invalid(context, group, c))
    {
        return true;
    }
    matches!(name, "sed" | "perl") && rewrites_invalid(context, name, &values)
}

pub fn writes_invalid(context: &Context, command: &str, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let Some(all) = tokens(command) else {
        return false;
    };
    let everything: Vec<&Token> = all.iter().collect();
    for group in split(&everything, &CONTROLS) {
        let stages = split(&group, &PIPELINES);
        let parsed: Vec<Option<Stage>> = stages.iter().map(|s| parse_stage(s)).collect();
        if parsed.iter().any(Option::is_none) {
            continue;
        }
        for (stage, parsed) in stages.iter().zip(parsed.iter().flatten()) {
            if stage_invalid(context, command, &group, stage, parsed, depth) {
                return true;
            }
        }
    }
    false
}
