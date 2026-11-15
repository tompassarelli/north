//! A Bash call's stages, from the shared shell lexer: the program each stage runs after any
//! wrapper, its arguments, and whether it loops or runs in the background.
use crate::shell::{basename, command, is_separator, words};

const WRAPPERS: &[&str] = &["env", "exec", "time", "nice", "timeout", "builtin"];
const KEYWORDS: &[&str] = &["do", "then", "else", "{", "!"];

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Stage {
    pub program: String,
    pub args: Vec<String>,
    pub backgrounded: bool,
    pub loop_keyword: bool,
}

pub fn tokens(text: &str) -> Vec<String> {
    words(text).unwrap_or_else(|| text.split_whitespace().map(str::to_owned).collect())
}

pub fn stages(text: &str) -> Vec<Stage> {
    let tokens = tokens(text);
    let mut out = Vec::new();
    let mut current = Vec::new();
    for token in &tokens {
        if is_separator(token) {
            if let Some(stage) = stage(&current, token == "&") {
                out.push(stage);
            }
            current.clear();
        } else {
            current.push(token.clone());
        }
    }
    out.extend(stage(&current, false));
    out
}

fn stage(segment: &[String], backgrounded: bool) -> Option<Stage> {
    let skip = segment.iter().take_while(|word| KEYWORDS.contains(&word.as_str())).count();
    let segment = &segment[skip..];
    let start = command(segment, WRAPPERS);
    let (program, args) = segment.get(start..)?.split_first()?;
    let program = basename(program).to_owned();
    let loop_keyword = matches!(program.as_str(), "while" | "until" | "for");
    Some(Stage { program, args: args.to_vec(), backgrounded, loop_keyword })
}

pub fn writes(text: &str) -> Vec<String> {
    redirect_targets(text, ">")
}

pub fn appends(text: &str) -> Vec<String> {
    redirect_targets(text, ">>")
}

fn redirect_targets(text: &str, operator: &str) -> Vec<String> {
    let tokens = tokens(text);
    let mut targets = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let rest = token.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
        let Some(target) = rest.strip_prefix(operator) else { continue };
        if target.starts_with('>') || target.starts_with('&') {
            continue;
        }
        let target = if target.is_empty() { tokens.get(index + 1).map(String::as_str) } else { Some(target) };
        targets.extend(target.filter(|target| !target.is_empty()).map(str::to_owned));
    }
    targets
}
