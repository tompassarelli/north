use super::{ASSIGNMENT, SHELLS, base};
use regex::Regex;
use std::sync::LazyLock;

static HEREDOC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(^|[^<])<<(-?)\s*(\\?)(["']?)([A-Za-z_][A-Za-z0-9_]*)"#).unwrap()
});
static PIPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\|\s*(sudo\s+)?(bash|sh|zsh|dash|ksh)(\s|$)").unwrap());
static FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|[^0-9&<>])>>?\s*([^\s;|&<>]+)").unwrap());

struct Opener {
    delimiter: String,
    dash: bool,
    unquoted: bool,
    executed: bool,
    file: String,
}

struct Body {
    start: usize,
    end: usize,
    executed: bool,
    file: String,
}

fn unquote(word: &str) -> &str {
    let w = word.strip_prefix(['"', '\'']).unwrap_or(word);
    w.strip_suffix(['"', '\'']).unwrap_or(w)
}

fn wrapper(word: &str) -> bool {
    matches!(
        word,
        "sudo" | "env" | "command" | "exec" | "nohup" | "time" | "nice" | "stdbuf" | "builtin"
    ) || word.starts_with('-')
        || ASSIGNMENT.is_match(word)
}

fn command_word<'a>(words: &[&'a str]) -> (&'a str, &'a str) {
    let mut found = "";
    let mut skip = false;
    for word in words {
        if skip {
            skip = false;
            continue;
        }
        let w = unquote(word);
        if !found.is_empty() {
            if !w.starts_with('-') {
                return (found, base(w));
            }
            continue;
        }
        if wrapper(w) {
            continue;
        }
        if w == "timeout" {
            skip = true;
            continue;
        }
        found = base(w);
    }
    (found, "")
}

fn runs_file(line: &str, file: &str) -> bool {
    line.split([';', '|', '&', '(', ')', '`']).any(|segment| {
        let words: Vec<_> = segment.split_whitespace().collect();
        let (word, argument) = command_word(&words);
        if SHELLS.contains(&word) {
            argument == file
        } else {
            word == file
        }
    })
}

fn opener(captures: &regex::Captures<'_>, before: &str, after: &str) -> Opener {
    let segment = before
        .rsplit([';', '|', '&', '(', '`'])
        .next()
        .unwrap_or("");
    let tail = after.split([';', '|', '&']).next().unwrap_or("");
    let words: Vec<_> = segment.split_whitespace().collect();
    let (word, argument) = command_word(&words);
    let file = if word == "tee" {
        argument.to_owned()
    } else {
        FILE.captures(&format!("{segment} {tail}"))
            .map(|m| base(m[2].trim_matches(['"', '\''])).to_owned())
            .unwrap_or_default()
    };
    Opener {
        delimiter: captures[5].to_owned(),
        dash: &captures[2] == "-",
        unquoted: captures[3].is_empty() && captures[4].is_empty(),
        executed: (!word.is_empty() && SHELLS.contains(&word)) || PIPE.is_match(after),
        file,
    }
}

fn openers(line: &str) -> Vec<Opener> {
    let mut rest = line;
    let mut before = String::new();
    let mut found = Vec::new();
    while let Some(captures) = HEREDOC.captures(rest) {
        let whole = captures.get(0).unwrap();
        before.push_str(&rest[..whole.start()]);
        before.push_str(&captures[1]);
        let after = rest[whole.end()..]
            .strip_prefix(['"', '\''])
            .unwrap_or(&rest[whole.end()..]);
        found.push(opener(&captures, &before, after));
        before.push_str(&whole.as_str()[captures[1].len()..]);
        rest = after;
    }
    found
}

fn read_body(lines: &[&str], index: &mut usize, opener: Opener) -> Body {
    let start = *index;
    let mut text = String::new();
    while *index < lines.len() {
        let line = if opener.dash {
            lines[*index].trim_start_matches('\t')
        } else {
            lines[*index]
        };
        *index += 1;
        if line == opener.delimiter {
            break;
        }
        text.push_str(line);
        text.push('\n');
    }
    let substitutes = opener.unquoted && (text.contains("$(") || text.contains('`'));
    Body {
        start,
        end: index.saturating_sub(1),
        executed: opener.executed || substitutes,
        file: opener.file,
    }
}

fn data_line(bodies: &[Body], line: usize) -> bool {
    bodies
        .iter()
        .any(|b| !b.executed && (b.start..=b.end).contains(&line))
}

fn mark_run_files(lines: &[&str], bodies: &mut [Body]) {
    for h in 0..bodies.len() {
        if bodies[h].executed || bodies[h].file.is_empty() {
            continue;
        }
        let ran = lines
            .iter()
            .enumerate()
            .skip(bodies[h].start - 1)
            .any(|(j, line)| {
                !(bodies[h].start..=bodies[h].end).contains(&j)
                    && !data_line(bodies, j)
                    && runs_file(line, &bodies[h].file)
            });
        bodies[h].executed = ran;
    }
}

pub(super) fn strip_data_bodies(cmd: &str) -> String {
    if !cmd.contains("<<") {
        return cmd.to_owned();
    }
    let lines: Vec<_> = cmd.split_terminator('\n').collect();
    let mut bodies = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let found = openers(lines[i]);
        i += 1;
        for opener in found {
            bodies.push(read_body(&lines, &mut i, opener));
        }
    }
    mark_run_files(&lines, &mut bodies);
    lines
        .iter()
        .enumerate()
        .filter(|(j, _)| !data_line(&bodies, *j))
        .map(|(_, s)| *s)
        .collect::<Vec<_>>()
        .join("\n")
}
