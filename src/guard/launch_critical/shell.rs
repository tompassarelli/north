use super::{ASSIGNMENT, BLOCK_CLOSE, BLOCK_OPEN, CONTROL_WORDS, Env, Policy};
use crate::guard::paths;
use crate::shell::{self, basename, shell_substitutions, unquoted_mask, whitespace};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

pub(super) fn tokens(text: &str) -> Vec<String> {
    shell::shlex_split(text).unwrap_or_else(|| {
        text.split(whitespace)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    })
}
pub(super) fn resolve(path: &str, cwd: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    paths::realpath(&Path::new(cwd).join(path))
}

pub(super) fn redirect_targets(text: &str) -> Vec<String> {
    let mask: Vec<_> = unquoted_mask(text).chars().collect();
    let chars: Vec<_> = text.chars().collect();
    static TARGET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"^[\s\x1c-\x1f]*("[^"]+"|'[^']+'|[^\s\x1c-\x1f;&|<>]+)"#).unwrap()
    });
    let mut targets = Vec::new();
    let mut i = 0;
    while i < mask.len() {
        if mask[i] != '>' || (i > 0 && ['-', '&'].contains(&mask[i - 1])) {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        if mask.get(end) == Some(&'>') {
            end += 1;
        }
        if mask.get(end) == Some(&'&') {
            if end == i + 2 {
                end -= 1;
            } else {
                i += 1;
                continue;
            }
        }
        let tail: String = chars[end..].iter().collect();
        if let Some(m) = TARGET.captures(&tail) {
            targets.push(m[1].trim_matches(['"', '\'']).to_owned());
        }
        i = end;
    }
    for body in shell_substitutions(text) {
        targets.extend(redirect_targets(&body));
    }
    targets
}
pub(super) fn record_assignments(words: &[String], env: &mut Env, policy: &Policy) {
    let index = words
        .iter()
        .position(|s| !CONTROL_WORDS.contains(&s.as_str()))
        .unwrap_or(words.len());
    let rest = &words[index..];
    if rest.len() > 1 && rest[0] == "for" {
        env.insert(rest[1].clone(), None);
        return;
    }
    if rest.first().is_some_and(|s| s == "read") {
        static NAME: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());
        for word in &rest[1..] {
            if NAME.is_match(word) {
                env.insert(word.clone(), None);
            }
        }
        return;
    }
    let assignments: Vec<_> = if rest.first().is_some_and(|s| {
        ["export", "local", "declare", "readonly", "typeset"].contains(&s.as_str())
    }) {
        rest[1..].iter().filter(|s| !s.starts_with('-')).collect()
    } else {
        rest.iter().collect()
    };
    if assignments.is_empty() || assignments.iter().any(|s| !ASSIGNMENT.is_match(s)) {
        return;
    }
    for word in assignments {
        let m = ASSIGNMENT.captures(word).unwrap();
        let value = policy.substitute(&m[2], env);
        env.insert(m[1].into(), (!value.contains(['$', '`'])).then_some(value));
    }
}
pub(super) fn union(first: &[String], second: &[String]) -> Vec<String> {
    let mut all = first.to_vec();
    for value in second {
        if !all.contains(value) {
            all.push(value.clone());
        }
    }
    all
}
pub(super) struct Segment {
    pub(super) text: String,
    pub(super) env: Env,
    pub(super) cwds: Vec<String>,
}
impl Policy {
    fn cd_destinations(&self, words: &[String], env: &Env, cwds: &[String]) -> Vec<String> {
        let i = words
            .iter()
            .position(|s| !CONTROL_WORDS.contains(&s.as_str()))
            .unwrap_or(words.len());
        if i == words.len() || !["cd", "pushd"].contains(&words[i].as_str()) {
            return Vec::new();
        }
        let mut args = &words[i + 1..];
        while args.first().is_some_and(|s| s.starts_with('-') && s != "-") {
            let done = args[0] == "--";
            args = &args[1..];
            if done {
                break;
            }
        }
        let Some(arg) = args.first() else {
            return self
                .base_env()
                .get("HOME")
                .and_then(Clone::clone)
                .into_iter()
                .collect();
        };
        if arg == "-" || arg.starts_with('+') {
            return Vec::new();
        }
        let mut moved = Vec::new();
        for here in cwds {
            let Some((path, scope)) = self.target(arg, env, here) else {
                return Vec::new();
            };
            if path != scope {
                return Vec::new();
            }
            if !moved.contains(&path) {
                moved.push(path);
            }
        }
        moved
    }
    pub(super) fn segments(&self, text: &str, initial: &Env, cwd: &str) -> Vec<Segment> {
        let mut env = initial.clone();
        let mut cwds = vec![cwd.to_owned()];
        let mut stack = Vec::new();
        let mut prev_sep: Option<String> = None;
        let mut out = Vec::new();
        for (text, sep) in shell::shell_segments_sep(text) {
            let tokens = tokens(&text);
            let mask = unquoted_mask(&text);
            let lead = text.len() - text.trim_start_matches(['(', ' ']).len();
            let opens = mask[..lead].matches('(').count();
            for _ in 0..opens {
                stack.push((true, cwds.clone()));
            }
            let words: Vec<_> = tokens
                .iter()
                .map(|s| s.trim_start_matches('(').to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            if words
                .first()
                .is_some_and(|s| BLOCK_OPEN.contains(&s.as_str()))
            {
                stack.push((false, cwds.clone()));
            }
            out.push(Segment {
                text,
                env: env.clone(),
                cwds: cwds.clone(),
            });
            record_assignments(&tokens, &mut env, self);
            if prev_sep.as_deref() != Some("|") && !matches!(sep.as_deref(), Some("|" | "&")) {
                let moved = self.cd_destinations(&words, &env, &cwds);
                if !moved.is_empty() {
                    cwds = if sep.as_deref() == Some("&&") {
                        moved
                    } else {
                        union(&cwds, &moved)
                    };
                }
            }
            let net = mask.matches('(').count() as isize
                - opens as isize
                - mask.matches(')').count() as isize;
            for _ in 0..net.max(0) {
                stack.push((true, cwds.clone()));
            }
            for _ in 0..(-net).max(0) {
                if let Some((subshell, saved)) = stack.pop() {
                    cwds = if subshell {
                        saved
                    } else {
                        union(&saved, &cwds)
                    };
                }
            }
            if words
                .first()
                .is_some_and(|s| BLOCK_CLOSE.contains(&s.as_str()))
                && let Some((_, saved)) = stack.pop() {
                    cwds = union(&saved, &cwds);
                }
            prev_sep = sep;
        }
        out
    }
}
pub(super) fn leading_words(segment: &str) -> Vec<String> {
    static ASSIGN_PREFIX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z_0-9]*=").unwrap());
    tokens(segment)
        .into_iter()
        .skip_while(|word| ASSIGN_PREFIX.is_match(word) || ["env", "exec", "command"].contains(&word.as_str()))
        .collect()
}
pub(super) fn leads_with(segment: &str, name: &str) -> bool {
    leading_words(segment).first().is_some_and(|word| basename(word) == name)
}
pub(super) fn leads_with_rescue(segment: &str) -> bool {
    let words = leading_words(segment);
    words.first().is_some_and(|word| basename(word) == "north") && words.get(1).is_some_and(|word| word == "wt-rescue")
}
pub(super) fn shell_c_script(words: &[String], executable: usize) -> Option<&str> {
    let mut i = executable + 1;
    while i < words.len() {
        let arg = &words[i];
        if ["-O", "-o"].contains(&arg.as_str()) && i + 1 < words.len() {
            i += 2;
            continue;
        }
        if arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains('c') {
            i += 1;
            if words.get(i).is_some_and(|s| s == "--") {
                i += 1;
            }
            return words.get(i).map(String::as_str);
        }
        if arg == "--" || arg.starts_with('-') {
            i += 1;
            continue;
        }
        return None;
    }
    None
}
