use std::collections::HashSet;

pub const BOUND: usize = 1500;

const FALLBACK_LINES: usize = 3;

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

fn wanted(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut lines = text.lines().map(str::trim_end).peekable();
    let mut in_test = false;
    while let Some(line) = lines.next() {
        if line.starts_with("---- ") && line.ends_with(" stdout ----") {
            found.push(line);
            in_test = true;
        } else if in_test && line.starts_with("thread '") && line.contains(" panicked at ") {
            found.push(line);
            if line.ends_with(':')
                && let Some(message) = lines.next_if(|next| !next.trim().is_empty())
            {
                found.push(message);
            }
            in_test = false;
        } else if line.starts_with("safe-push: ✗") || gate(line) || exited(line) {
            found.push(line);
        }
    }
    if found.is_empty() {
        let tail: Vec<&str> = text.lines().map(str::trim_end).filter(|line| !line.is_empty() && !line.starts_with("safe-push: verdict ")).collect();
        found = tail[tail.len().saturating_sub(FALLBACK_LINES)..].to_vec();
    }
    found
}

pub fn reason(text: &str) -> String {
    let mut seen = HashSet::new();
    let mut reason = String::new();
    for line in wanted(text).into_iter().filter(|line| seen.insert(*line)) {
        let room = BOUND.saturating_sub(reason.len() + usize::from(!reason.is_empty()));
        if !reason.is_empty() {
            reason.push('\n');
        }
        if line.len() <= room {
            reason.push_str(line);
            continue;
        }
        let mut cut = room.saturating_sub('…'.len_utf8());
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        reason.push_str(&line[..cut]);
        reason.push('…');
        break;
    }
    reason
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_gate_safe_push_and_panic_lines_in_order_once() {
        let log = "Compiling x\ndep-gate: crates/x/Cargo.toml: proptest: added dependency needs a reason\nrunning 2 tests\n---- a::b stdout ----\nnoise\nthread 'a::b' panicked at src/a.rs:3:5:\nassertion failed: ok\nnote: run with `RUST_BACKTRACE=1`\nfailures:\n    a::b\nland check: cargo test exited 101 after 12s\nsafe-push: land check cargo test exited 101; log: /tmp/x; NOT pushed.\nsafe-push: ✗ the pre-push checks failed; NOT pushed.\nsafe-push: ✗ the pre-push checks failed; NOT pushed.\nsafe-push: verdict refused /tmp/x\n";
        assert_eq!(
            reason(log),
            "dep-gate: crates/x/Cargo.toml: proptest: added dependency needs a reason\n---- a::b stdout ----\nthread 'a::b' panicked at src/a.rs:3:5:\nassertion failed: ok\nland check: cargo test exited 101 after 12s\nsafe-push: land check cargo test exited 101; log: /tmp/x; NOT pushed.\nsafe-push: ✗ the pre-push checks failed; NOT pushed."
        );
    }

    #[test]
    fn falls_back_to_the_last_lines_and_stays_within_the_bound() {
        assert_eq!(reason("a\nb\n\nc\nd\nsafe-push: verdict refused x\n"), "b\nc\nd");
        let long: String = (0..400).map(|n| format!("safe-push: ✗ failure é{n}\n")).collect();
        let cut = reason(&long);
        assert!(cut.len() <= BOUND && cut.len() > BOUND - 40 && cut.ends_with('…'), "{}", cut.len());
    }
}
