use crate::shell;
use std::time::Duration;

const LANDING: &str = "main";

pub(super) fn rewrite(command: &str, cwd: &str) -> Option<String> {
    let chars: Vec<char> = command.chars().collect();
    let pieces = shell::pieces(&shell::strip_heredocs(command))?;
    let segments = shell::piece_segments(&pieces);
    let mut words = Vec::new();
    for segment in &segments {
        let mut line = Vec::new();
        for piece in segment {
            let source = shell::slice(&chars, *piece);
            match shell::shlex_split(&source).as_deref() {
                Some([word]) => line.push((*piece, word.clone(), source)),
                _ => return None,
            }
        }
        words.push(line);
    }
    let moves = words.iter().any(|line| line.first().is_some_and(|(_, word, _)| word == "cd"));
    let mut edits = Vec::new();
    for line in &words {
        let (values, redirects) = split_redirects(line);
        if values.starts_with(&["git", "push"]) {
            let mut replacement = replacement(&values[2..], cwd, moves)?;
            for redirect in redirects {
                replacement.push(' ');
                replacement.push_str(redirect);
            }
            let start = line.first()?.0.start;
            let end = line.last()?.0.end;
            edits.push((start, end, replacement));
        }
    }
    (!edits.is_empty()).then(|| shell::splice(&chars, edits))
}

fn replacement(args: &[&str], cwd: &str, moves: bool) -> Option<String> {
    let operands: Vec<&str> = args
        .iter()
        .copied()
        .filter(|arg| !matches!(*arg, "-u" | "--set-upstream"))
        .collect();
    if operands.iter().any(|arg| arg.starts_with('-')) {
        return None;
    }
    match operands.as_slice() {
        ["origin", refspec] if refspec.starts_with("HEAD:") => {
            let branch = refspec.strip_prefix("HEAD:")?;
            let plain = !branch.is_empty()
                && branch.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-./".contains(&b))
                && !branch.starts_with("refs/");
            plain.then(|| format!("safe-push --to {branch}"))
        }
        [] | ["origin"] | ["origin", LANDING | "HEAD"] if !moves && on_landing_branch(cwd) => {
            Some("safe-push".into())
        }
        _ => None,
    }
}

fn on_landing_branch(cwd: &str) -> bool {
    crate::platform::git::run_within(&["-C", cwd, "symbolic-ref", "--short", "HEAD"], Duration::from_secs(5))
        .filter(|output| output.status.success())
        .is_some_and(|output| String::from_utf8_lossy(&output.stdout).trim() == LANDING)
}

fn split_redirects(line: &[(shell::Piece, String, String)]) -> (Vec<&str>, Vec<&str>) {
    let (mut values, mut redirects) = (Vec::new(), Vec::new());
    let mut operator = false;
    for (_, word, source) in line {
        let bare = source.trim_start_matches(|c: char| c.is_ascii_digit());
        if operator || bare.starts_with(['>', '<']) {
            operator = !operator && bare.chars().all(|c| matches!(c, '>' | '<'));
            redirects.push(source.as_str());
        } else {
            values.push(word.as_str());
        }
    }
    (values, redirects)
}
