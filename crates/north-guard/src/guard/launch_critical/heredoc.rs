use super::{Env, Policy};
use regex::Regex;
use std::sync::LazyLock;

static WRITES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"write|\.save|rename|unlink|remove|rmtree|copy|move|mkdir|chmod|symlink|truncate|subprocess|system\(|exec|spawn|\btee\b|\bsed\b|\bcp\b|\bmv\b|\brm\b|>|\bopen\s*\([^()]*,\s*(?:mode\s*=\s*)?['"][rbt]*[wax+]"#).unwrap()
});
static LITERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"'([^'\n]*)'|"([^"\n]*)""#).unwrap());
static PATH_LIKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w.~$/{}-]+$").unwrap());
static EXTENSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.[A-Za-z][A-Za-z0-9]{0,4}$").unwrap());

impl Policy {
    pub(super) fn heredoc_confined(&self, bodies: &[&str], arguments: &[String], env: &Env, cwd: &str) -> bool {
        if !bodies.iter().any(|body| WRITES.is_match(body)) {
            return true;
        }
        let literals = bodies.iter().flat_map(|body| LITERAL.captures_iter(body).filter_map(|c| c.get(1).or_else(|| c.get(2))).map(|m| m.as_str()));
        arguments
            .iter()
            .map(String::as_str)
            .chain(literals)
            .filter(|word| PATH_LIKE.is_match(word) && (word.contains('/') || EXTENSION.is_match(word)))
            .all(|word| word.starts_with(['/', '~']) && self.target(word, env, cwd).is_none_or(|(path, _)| self.protected(&path).is_none()))
    }
}
