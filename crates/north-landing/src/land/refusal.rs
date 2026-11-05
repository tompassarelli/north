use std::collections::HashSet;

pub const BOUND: usize = 3000;
pub const ERROR_LINES: usize = 20;
pub const TAIL_LINES: usize = 20;

const TAIL_BOUND: usize = BOUND / 2;
const LINE_BOUND: usize = 300;
const TAIL_HEADER: &str = "last lines of the log:";

pub(super) fn gate(line: &str) -> bool {
    line.split_once(": ").is_some_and(|(name, _)| !name.contains(' ') && (name.ends_with("-gate") || name.ends_with("-guard")))
}

fn exited(line: &str) -> bool {
    line.match_indices("exited ").any(|(at, word)| {
        let rest = &line[at + word.len()..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        digits > 0 && (rest[digits..].starts_with(" after") || rest[digits..].starts_with(';'))
    })
}

fn compiler_error(line: &str) -> bool {
    line.strip_prefix("error").is_some_and(|rest| rest.starts_with(':') || rest.starts_with('['))
}

fn test_failure(line: &str) -> bool {
    (line.starts_with("test ") && line.ends_with(" FAILED")) || line.trim_start().starts_with("FAIL ")
}

fn wanted(text: &str) -> Vec<(&str, bool)> {
    let mut found = Vec::new();
    let mut lines = text.lines().map(str::trim_end).peekable();
    let mut in_test = false;
    while let Some(line) = lines.next() {
        if line.starts_with("---- ") && line.ends_with(" stdout ----") {
            found.push((line, false));
            in_test = true;
        } else if in_test && line.starts_with("thread '") && line.contains(" panicked at ") {
            found.push((line, false));
            if line.ends_with(':')
                && let Some(message) = lines.next_if(|next| !next.trim().is_empty())
            {
                found.push((message, false));
            }
            in_test = false;
        } else if compiler_error(line) {
            found.push((line, true));
            while let Some(next) = lines.next_if(|next| !next.trim().is_empty()) {
                found.push((next, true));
            }
        } else if line.starts_with("safe-push: ✗") || gate(line) || exited(line) || test_failure(line) {
            found.push((line, false));
        }
    }
    found
}

fn clip(line: &str, room: usize) -> String {
    if line.len() <= room {
        return line.to_owned();
    }
    let mut cut = room.saturating_sub('…'.len_utf8());
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &line[..cut])
}

fn fit(lines: &[&str], bound: usize) -> String {
    let mut text = String::new();
    for line in lines {
        let room = bound.saturating_sub(text.len() + usize::from(!text.is_empty()));
        if !text.is_empty() {
            text.push('\n');
        }
        if line.len() <= room {
            text.push_str(line);
            continue;
        }
        text.push_str(&clip(line, room));
        break;
    }
    text
}

fn first_errors(text: &str) -> Vec<&str> {
    let mut seen = HashSet::new();
    wanted(text).into_iter().filter(|(line, block)| *block || seen.insert(*line)).map(|(line, _)| line).take(ERROR_LINES).collect()
}

/// A line Nix prints for an error it ignored itself, such as a busy eval cache, which never explains a failure.
fn nix_ignored(line: &str) -> bool {
    line.trim_start().split_once(" (ignored): ").is_some_and(|(level, _)| level == "error" || level == "warning")
}

fn last_lines(text: &str) -> Vec<&str> {
    let kept: Vec<&str> = text.lines().map(str::trim_end).filter(|line| !line.is_empty() && !line.starts_with("safe-push: verdict ") && !nix_ignored(line)).collect();
    kept[kept.len().saturating_sub(TAIL_LINES)..].to_vec()
}

fn tail(text: &str, shown: &HashSet<&str>) -> String {
    let mut room = TAIL_BOUND.saturating_sub(TAIL_HEADER.len() + 1);
    let mut lines = Vec::new();
    for line in last_lines(text).into_iter().rev().filter(|line| !shown.contains(line)) {
        if room < 2 {
            break;
        }
        let line = clip(line, (room - 1).min(LINE_BOUND));
        room -= line.len() + 1;
        lines.push(line);
    }
    lines.reverse();
    if lines.is_empty() { String::new() } else { format!("{TAIL_HEADER}\n{}", lines.join("\n")) }
}

pub fn errors(text: &str) -> String {
    let found = first_errors(text);
    fit(&if found.is_empty() { last_lines(text) } else { found }, BOUND)
}

pub fn reason(text: &str) -> String {
    let found = first_errors(text);
    let shown: HashSet<&str> = found.iter().copied().collect();
    let tail = tail(text, &shown);
    let separator = usize::from(!tail.is_empty() && !found.is_empty());
    let errors = fit(&found, BOUND.saturating_sub(tail.len() + separator));
    [errors, tail].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n")
}
