use std::collections::HashSet;

pub const BOUND: usize = 3000;
pub const ERROR_LINES: usize = 20;
pub const TAIL_LINES: usize = 20;

const TAIL_BOUND: usize = BOUND / 2;
const LINE_BOUND: usize = 300;
const TAIL_HEADER: &str = "last lines of the log:";

fn gate(line: &str) -> bool {
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

fn last_lines(text: &str) -> Vec<&str> {
    let kept: Vec<&str> = text.lines().map(str::trim_end).filter(|line| !line.is_empty() && !line.starts_with("safe-push: verdict ")).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_gate_safe_push_and_panic_lines_in_order_once_then_the_log_tail() {
        let log = "Compiling x\ndep-gate: crates/x/Cargo.toml: proptest: added dependency needs a reason\nrunning 2 tests\n---- a::b stdout ----\nnoise\nthread 'a::b' panicked at src/a.rs:3:5:\nassertion failed: ok\nnote: run with `RUST_BACKTRACE=1`\nfailures:\n    a::b\nland check: cargo test exited 101 after 12s\nsafe-push: land check cargo test exited 101; log: /tmp/x; NOT pushed.\nsafe-push: ✗ the pre-push checks failed; NOT pushed.\nsafe-push: ✗ the pre-push checks failed; NOT pushed.\nsafe-push: verdict refused /tmp/x\n";
        assert_eq!(
            reason(log),
            "dep-gate: crates/x/Cargo.toml: proptest: added dependency needs a reason\n---- a::b stdout ----\nthread 'a::b' panicked at src/a.rs:3:5:\nassertion failed: ok\nland check: cargo test exited 101 after 12s\nsafe-push: land check cargo test exited 101; log: /tmp/x; NOT pushed.\nsafe-push: ✗ the pre-push checks failed; NOT pushed.\nlast lines of the log:\nCompiling x\nrunning 2 tests\nnoise\nnote: run with `RUST_BACKTRACE=1`\nfailures:\n    a::b"
        );
    }

    #[test]
    fn keeps_rustc_and_clippy_error_blocks_and_failed_tests() {
        let log = "warning: unused\n --> a.rs:1:1\n\nerror: this `if` statement can be collapsed\n --> src/lib.rs:3:5\n  |\n3 |     if a { if b {} }\n  |\n  = help: for further information visit https://rust-lang.github.io/rust-clippy\n\nerror: could not compile `x` (lib) due to 1 previous error\ntest a::b ... FAILED\n";
        let found = errors(log);
        assert_eq!(found.lines().count(), 8, "{found}");
        assert!(found.starts_with("error: this `if` statement can be collapsed\n --> src/lib.rs:3:5") && found.ends_with("test a::b ... FAILED"), "{found}");
        assert!(!found.contains("unused"), "{found}");
    }

    #[test]
    fn keeps_the_first_twenty_error_lines_and_the_last_twenty_log_lines() {
        let log: String = (0..40).map(|n| format!("error: failure {n}\n\n")).chain((0..40).map(|n| format!("tail {n}\n"))).collect();
        let found = errors(&log);
        assert_eq!(found.lines().count(), ERROR_LINES, "{found}");
        assert!(found.ends_with("error: failure 19"), "{found}");
        let told = reason(&log);
        let (head, tail) = told.split_once("\nlast lines of the log:\n").unwrap();
        assert_eq!(head.lines().count(), ERROR_LINES);
        assert_eq!(tail.lines().count(), TAIL_LINES);
        assert!(tail.starts_with("tail 20\n") && tail.ends_with("tail 39"), "{tail}");
    }

    #[test]
    fn falls_back_to_the_last_lines_and_stays_within_the_bound() {
        assert_eq!(errors("a\nb\n\nc\nd\nsafe-push: verdict refused x\n"), "a\nb\nc\nd");
        let long: String = (0..400).map(|n| format!("safe-push: ✗ failure é{n} {}\n", "x".repeat(60))).collect();
        let cut = reason(&long);
        assert!(cut.len() <= BOUND && cut.len() > BOUND - 40 && cut.contains("\nlast lines of the log:\n"), "{}", cut.len());
        assert!(errors(&long).len() <= BOUND);
    }
}
