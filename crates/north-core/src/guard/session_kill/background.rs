use regex::Regex;
use super::{LAUNCH_PREFIXES, UNMANAGED, launch};
use crate::shell::{self, Piece, PieceKind};

const BOUND: &str = "24h";
const NOT_EXECUTABLE: &[&str] = &[
    "!", "{", "}", "[[", "if", "then", "else", "elif", "fi", "for", "while", "until", "do",
    "done", "case", "esac", "in", "function", "time", "coproc", "exec", "cd", "pushd", "popd",
    "export", "source", ".", "eval", "set", "unset", "alias", "wait", "trap", "read", "local",
    "declare", "typeset", "readonly", "shift", "return", "exit", "umask", "ulimit", "builtin",
    "command", "hash", "jobs", "fg", "bg", "disown", "true", "false", ":", "echo", "printf",
    "test", "[", "kill", "type", "pwd", "setsid",
];

pub(super) fn rewrite(command: &str) -> Option<String> {
    let chars: Vec<char> = command.chars().collect();
    let pieces = shell::pieces(&shell::strip_heredocs(command))?;
    let mut edits = Vec::new();
    let mut segment: Vec<Piece> = Vec::new();
    let mut piped = false;
    let (mut opens, mut closed): (Vec<usize>, Option<(usize, usize)>) = (Vec::new(), None);
    for (index, piece) in pieces.iter().enumerate() {
        if piece.kind == PieceKind::Word {
            segment.push(*piece);
            continue;
        }
        let background = backgrounds(&chars, &pieces, index);
        match chars[piece.start] {
            '(' => opens.push(piece.start),
            ')' => closed = opens.pop().map(|open| (open, piece.end)),
            _ => {}
        }
        if background && !piped && segment.is_empty() && let Some((open, close)) = closed.filter(|(_, close)| pieces[index - 1].end == *close) {
            let group = group_edit(&chars, open, close)?;
            edits.retain(|(start, _, _): &(usize, usize, String)| *start < open || *start >= close);
            edits.push(group);
        }
        edits.extend(wrap(&chars, &segment, background && !piped)?);
        piped = chars[piece.start] == '|';
        segment.clear();
    }
    edits.extend(wrap(&chars, &segment, false)?);
    (!edits.is_empty()).then(|| shell::splice(&chars, edits))
}

fn separator(chars: &[char], pieces: &[Piece], index: usize, wanted: char) -> bool {
    pieces
        .get(index)
        .is_some_and(|piece| piece.kind == PieceKind::Sep && chars[piece.start] == wanted)
}

fn backgrounds(chars: &[char], pieces: &[Piece], index: usize) -> bool {
    if chars[pieces[index].start] != '&' {
        return false;
    }
    let touches = |other: Option<usize>, wanted: char, after: bool| {
        other.is_some_and(|other| {
            separator(chars, pieces, other, wanted)
                && if after {
                    pieces[other].start == pieces[index].end
                } else {
                    pieces[other].end == pieces[index].start
                }
        })
    };
    let before = index.checked_sub(1);
    !(touches(before, '&', false) || touches(before, '|', false) || touches(Some(index + 1), '&', true))
}

fn wrap(chars: &[char], segment: &[Piece], background: bool) -> Option<Option<(usize, usize, String)>> {
    if segment.is_empty() {
        return Some(None);
    }
    let mut values = Vec::new();
    let mut sources = Vec::new();
    for piece in segment {
        let source = shell::slice(chars, *piece);
        match shell::shlex_split(&source).as_deref() {
            Some([word]) => values.push(word.clone()),
            _ => return None,
        }
        sources.push(source);
    }
    let words = &values[shell::command(&values, LAUNCH_PREFIXES).min(values.len())..];
    let Some(shape) = launch(words, background) else {
        return Some(None);
    };
    let assigned = values
        .iter()
        .zip(&sources)
        .take_while(|(word, source)| shell::assignment(word) && !source.starts_with(['\'', '"']))
        .count();
    let command = values.get(assigned)?;
    let env = if assigned > 0 { "env" } else { "" };
    let prefix = format!("run-bounded {BOUND} --");
    let start = segment[0].start;
    if shape == "nohup" && command == "nohup" {
        let assignments = sources[..assigned].join(" ");
        let text = [prefix.as_str(), env, assignments.as_str()]
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        return Some(Some((start, segment[assigned].end, text)));
    }
    let wrappable = shape == UNMANAGED || shape.starts_with("direct ");
    let plain = !NOT_EXECUTABLE.contains(&command.as_str())
        && !sources[assigned].trim_start_matches(|c: char| c.is_ascii_digit()).starts_with(['<', '>']);
    if !wrappable || !plain {
        return None;
    }
    let text = if assigned > 0 { format!("{prefix} env ") } else { format!("{prefix} ") };
    Some(Some((start, start, text)))
}

const ENVIRONMENT: &[&str] = &["HOME", "PATH", "USER", "PWD", "TMPDIR"];

fn group_edit(chars: &[char], open: usize, close: usize) -> Option<(usize, usize, String)> {
    let body: String = chars[open + 1..close - 1].iter().collect();
    let reference = Regex::new(r"\$(?:\{([A-Za-z_][A-Za-z0-9_]*)|([A-Za-z_][A-Za-z0-9_]*))").unwrap();
    for found in reference.captures_iter(&body) {
        let name = found.get(1).or_else(|| found.get(2))?.as_str();
        let assigned = Regex::new(&format!(r"(?:^|[\s;&|(]){name}=")).unwrap().is_match(&body);
        if !assigned && !ENVIRONMENT.contains(&name) {
            return None;
        }
    }
    let quoted = body.trim().replace('\'', "'\"'\"'");
    Some((open, close, format!("run-bounded {BOUND} -- bash -c '{quoted}'")))
}
