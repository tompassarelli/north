use crate::platform::git::run_within;
use crate::shell;
use std::time::Duration;

const LIMIT: usize = 200;
const PASSTHROUGH: &[&str] = &["-q", "--quiet", "-n", "--no-verify", "-s", "--signoff"];

pub fn rewrite(command: &str, cwd: &str) -> Option<String> {
    let chars: Vec<char> = command.chars().collect();
    let pieces = shell::pieces(&shell::strip_heredocs(command))?;
    let segments = shell::piece_segments(&pieces);
    let mut edit = None;
    for segment in &segments {
        let sources: Vec<String> = segment.iter().map(|piece| shell::slice(&chars, *piece)).collect();
        let mut values = Vec::new();
        for source in &sources {
            match shell::shlex_split(source).as_deref() {
                Some([word]) => values.push(word.clone()),
                _ => return None,
            }
        }
        match values.first().map(String::as_str) {
            Some("cd" | "pushd") => return None,
            Some("git") if values.get(1).map(String::as_str) == Some("commit") => {
                if edit.is_some() {
                    return None;
                }
                let replacement = replacement(&values[2..], &sources[2..], cwd)?;
                edit = Some((segment.first()?.start, segment.last()?.end, replacement));
            }
            _ => {}
        }
    }
    edit.map(|edit| shell::splice(&chars, vec![edit]))
}

fn replacement(values: &[String], sources: &[String], cwd: &str) -> Option<String> {
    let (mut all, mut message, mut flags) = (false, None, Vec::new());
    let mut index = 0;
    while index < values.len() {
        let value = values[index].as_str();
        match value {
            "-a" | "--all" => all = true,
            "-am" | "-ma" => {
                all = true;
                index += 1;
                message = message.or(sources.get(index));
            }
            "-m" | "--message" => {
                index += 1;
                message = message.or(sources.get(index));
            }
            _ if PASSTHROUGH.contains(&value) => flags.push(sources[index].clone()),
            _ => return None,
        }
        index += 1;
    }
    let message = message?;
    if !all || message.starts_with('-') {
        return None;
    }
    let paths = modified_paths(cwd)?;
    let mut words = vec!["git commit".to_owned()];
    words.extend(flags);
    words.extend(["-m".to_owned(), message.clone(), "--".to_owned()]);
    words.extend(paths.iter().map(|path| quote(path)));
    Some(words.join(" "))
}

fn quote(word: &str) -> String {
    let plain = word
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\"'\"'"))
    }
}

fn git(cwd: &str, args: &[&str]) -> Option<Vec<u8>> {
    let mut full = vec!["-C", cwd];
    full.extend(args);
    run_within(&full, Duration::from_secs(5))
        .filter(|output| output.status.success())
        .map(|output| output.stdout)
}

fn modified_paths(cwd: &str) -> Option<Vec<String>> {
    if !git(cwd, &["rev-parse", "--show-prefix"])?.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    let status = git(cwd, &["status", "--porcelain=v1", "-z", "--untracked-files=no"])?;
    let mut paths = Vec::new();
    for entry in status.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
        let (flags, path) = entry.split_at(3.min(entry.len()));
        if flags.len() != 3 || flags[0] != b' ' || !matches!(flags[1], b'M' | b'D' | b'T') {
            return None;
        }
        paths.push(String::from_utf8(path.to_vec()).ok()?);
    }
    (!paths.is_empty() && paths.len() <= LIMIT).then_some(paths)
}
