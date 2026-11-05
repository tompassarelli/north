const SUBCOMMANDED: [&str; 11] = ["git", "cargo", "nix", "north", "gh", "bun", "npm", "systemctl", "journalctl", "docker", "safe-push"];
const WRAPPERS: [&[&str]; 5] = [&["depbox", "run", "--"], &["nix", "develop", "-c"], &["env"], &["time"], &["command"]];
const SKIPPED: [&str; 7] = ["cd", "echo", "true", "export", "set", "sleep", "printf"];
const SEPARATORS: [&str; 5] = ["&&", "||", ";", "|", "\n"];

pub fn command_of(text: &str) -> Option<&str> {
    let start = ["command=", "cmd="].iter().find_map(|key| text.strip_prefix(key))?;
    let end = start.find("\ndescription=").unwrap_or(start.len());
    Some(start[..end].trim()).filter(|command| !command.is_empty())
}

fn segments(command: &str) -> Vec<&str> {
    let mut pieces = vec![command];
    for separator in SEPARATORS {
        pieces = pieces.into_iter().flat_map(|piece| piece.split(separator)).collect();
    }
    pieces.into_iter().map(str::trim).filter(|piece| !piece.is_empty()).collect()
}

fn unwrapped<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    loop {
        let before = words.len();
        while words.first().is_some_and(|word| word.contains('=') && !word.starts_with('-')) {
            words = &words[1..];
        }
        if let Some(wrapper) = WRAPPERS.iter().find(|wrapper| words.starts_with(wrapper)) {
            words = &words[wrapper.len()..];
        }
        if words.len() == before {
            return words;
        }
    }
}

fn subcommand(word: &str) -> bool {
    word.bytes().next().is_some_and(|byte| byte.is_ascii_lowercase()) && word.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn step(segment: &str) -> Option<String> {
    let words: Vec<&str> = segment.split_whitespace().collect();
    let words = unwrapped(&words);
    let program = words.first()?.rsplit('/').next()?.trim_start_matches('(');
    if program.is_empty() || SKIPPED.contains(&program) || !program.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)) {
        return None;
    }
    let verb = SUBCOMMANDED.contains(&program).then(|| words[1..].iter().find(|word| !word.starts_with('-')).filter(|word| subcommand(word))).flatten();
    Some(verb.map_or_else(|| program.to_owned(), |verb| format!("{program} {verb}")))
}

pub fn steps(command: &str) -> Vec<String> {
    segments(command).into_iter().filter_map(step).collect()
}
