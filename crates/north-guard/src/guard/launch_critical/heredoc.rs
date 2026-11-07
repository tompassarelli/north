use super::{Env, Policy};
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

static WRITES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"write|\.save|rename|unlink|remove|rmtree|copy|move|mkdir|chmod|symlink|truncate|subprocess|system\(|exec|spawn|\btee\b|\bsed\b|\bcp\b|\bmv\b|\brm\b|>|\bopen\s*\([^()]*,\s*(?:mode\s*=\s*)?['"][rbt]*[wax+]"#).unwrap()
});
static LITERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"'([^'\n]*)'|"([^"\n]*)""#).unwrap());
static PATH_LIKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w.~$/{}-]+$").unwrap());
static EXTENSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.[A-Za-z][A-Za-z0-9]{0,4}$").unwrap());
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z_]\w*(\s*\()?").unwrap());
static ASSIGNED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\$?([A-Za-z_]\w*)\s*(?::[^=]*)?=\s*(.*)$").unwrap());
static ITERATED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*for\s+\$?([A-Za-z_]\w*)\s+in\s+(.*)$").unwrap());

fn variables(line: &str) -> BTreeSet<String> {
    let bare = LITERAL.replace_all(line, "''");
    WORD.find_iter(&bare)
        .filter(|m| !m.as_str().ends_with('(') && !bare[..m.start()].ends_with('.'))
        .map(|m| m.as_str().to_owned())
        .collect()
}
fn only_literals(text: &str) -> bool {
    LITERAL.replace_all(text, "").chars().all(|c| c.is_whitespace() || ",[](){}:;".contains(c))
}
fn literals(text: &str) -> impl Iterator<Item = &str> {
    LITERAL.captures_iter(text).filter_map(|c| c.get(1).or_else(|| c.get(2))).map(|m| m.as_str())
}

fn reaching_literals(body: &str) -> Vec<&str> {
    let lines: Vec<&str> = body.lines().collect();
    let mut written: BTreeSet<String> = lines.iter().filter(|line| WRITES.is_match(line)).flat_map(|line| variables(line)).collect();
    loop {
        let before = written.len();
        for line in &lines {
            if let Some(m) = ITERATED.captures(line)
                && written.contains(&m[1])
            {
                written.extend(variables(&m[2]));
            }
        }
        if written.len() == before {
            break;
        }
    }
    lines
        .iter()
        .flat_map(|line| {
            let direct = ASSIGNED
                .captures(line)
                .filter(|m| only_literals(&m[2]) && written.contains(&m[1]));
            let reaches = WRITES.is_match(line) || direct.is_some();
            literals(line).filter(move |_| reaches)
        })
        .collect()
}

impl Policy {
    pub(super) fn heredoc_confined(&self, bodies: &[&str], arguments: &[String], env: &Env, cwd: &str) -> bool {
        if !bodies.iter().any(|body| WRITES.is_match(body)) {
            return true;
        }
        let reaching: Vec<&str> = bodies.iter().flat_map(|body| reaching_literals(body)).collect();
        arguments
            .iter()
            .map(String::as_str)
            .chain(reaching)
            .filter(|word| PATH_LIKE.is_match(word) && (word.contains('/') || EXTENSION.is_match(word)))
            .all(|word| word.starts_with(['/', '~']) && self.target(word, env, cwd).is_none_or(|(path, _)| self.protected(&path).is_none()))
    }
}
