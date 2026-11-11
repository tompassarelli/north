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

pub struct Enumerated {
    pub command: String,
    pub commits: bool,
    pub untracked: Vec<String>,
}

pub fn enumerated(command: &str, cwd: &str) -> Option<Enumerated> {
    let chars: Vec<char> = command.chars().collect();
    let pieces = shell::pieces(&shell::strip_heredocs(command))?;
    let mut directory = std::path::PathBuf::from(cwd);
    let (mut entered, mut commits) = (String::new(), Vec::new());
    for segment in shell::piece_segments(&pieces) {
        let sources: Vec<String> = segment.iter().map(|piece| shell::slice(&chars, *piece)).collect();
        match (word(&sources, 0).as_deref(), word(&sources, 1).as_deref()) {
            (Some("cd" | "pushd"), target) => {
                directory.push(entered_directory(target.filter(|_| sources.len() == 2)?)?);
                entered.push_str(&format!("cd {} && ", sources[1]));
            }
            (Some("git"), Some("commit")) => commits.push(commit(&sources)),
            _ => {}
        }
    }
    let (tracked, untracked) = changes(directory.to_str()?)?;
    if tracked.is_empty() || tracked.len() > LIMIT {
        return None;
    }
    let commit = match commits.as_slice() {
        [Some(commit)] => format!(" && {commit}"),
        _ => String::new(),
    };
    Some(Enumerated {
        command: format!("{entered}git add -- {}{commit}", quoted(&tracked)),
        commits: !commit.is_empty(),
        untracked: untracked.iter().map(|path| quote(path)).collect(),
    })
}

fn word(sources: &[String], index: usize) -> Option<String> {
    match shell::shlex_split(sources.get(index)?).as_deref() {
        Some([word]) => Some(word.clone()),
        _ => None,
    }
}

fn entered_directory(target: &str) -> Option<std::path::PathBuf> {
    if target.contains(['$', '`']) {
        return None;
    }
    match target.strip_prefix("~/") {
        Some(rest) => Some(std::path::Path::new(&std::env::var_os("HOME")?).join(rest)),
        None => (!target.starts_with('~')).then(|| target.into()),
    }
}

fn commit(sources: &[String]) -> Option<String> {
    let values: Vec<String> = (2..sources.len()).map(|index| word(sources, index)).collect::<Option<_>>()?;
    let (_, flags, message) = options(&values, &sources[2..])?;
    let flags: String = flags.iter().map(|flag| format!("{flag} ")).collect();
    Some(format!("git commit {flags}-m {message}"))
}

fn quoted(paths: &[String]) -> String {
    paths.iter().map(|path| quote(path)).collect::<Vec<_>>().join(" ")
}


fn options(values: &[String], sources: &[String]) -> Option<(bool, Vec<String>, String)> {
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
    let message = message.filter(|message| !message.starts_with('-'))?;
    Some((all, flags, message.clone()))
}

fn replacement(values: &[String], sources: &[String], cwd: &str) -> Option<String> {
    let (all, flags, message) = options(values, sources)?;
    if !all {
        return None;
    }
    let paths = modified_paths(cwd)?;
    let mut words = vec!["git commit".to_owned()];
    words.extend(flags);
    words.extend(["-m".to_owned(), message, "--".to_owned()]);
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

fn changes(cwd: &str) -> Option<(Vec<String>, Vec<String>)> {
    let top = if git(cwd, &["rev-parse", "--show-prefix"])?.iter().all(u8::is_ascii_whitespace) { "" } else { ":/" };
    let status = git(cwd, &["status", "--porcelain=v1", "-z"])?;
    let (mut tracked, mut untracked) = (Vec::new(), Vec::new());
    let mut entries = status.split(|byte| *byte == 0).filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let (flags, path) = entry.split_at(3.min(entry.len()));
        let path = format!("{top}{}", String::from_utf8(path.to_vec()).ok()?);
        match change(flags)? {
            Change::Untracked => untracked.push(path),
            Change::Unstaged => tracked.push(path),
            Change::Staged => {}
        }
        if matches!(flags[0], b'R' | b'C') {
            entries.next();
        }
    }
    Some((tracked, untracked))
}

enum Change {
    Untracked,
    Unstaged,
    Staged,
}

fn change(flags: &[u8]) -> Option<Change> {
    match flags {
        b"?? " => Some(Change::Untracked),
        [b'U', ..] | [_, b'U', _] => None,
        [_, b'M' | b'D' | b'T', b' '] => Some(Change::Unstaged),
        [_, _, b' '] => Some(Change::Staged),
        _ => None,
    }
}
