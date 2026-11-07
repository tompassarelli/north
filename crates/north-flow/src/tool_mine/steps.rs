const SUBCOMMANDED: [&str; 11] = ["git", "cargo", "nix", "north", "gh", "bun", "npm", "systemctl", "journalctl", "docker", "safe-push"];
const REMOTE: &str = "ssh";
const WRAPPERS: [&[&str]; 5] = [&["depbox", "run", "--"], &["nix", "develop", "-c"], &["env"], &["time"], &["command"]];
const TIMEOUT_VALUED: [&str; 2] = ["-s", "-k"];
const INERT: [&str; 5] = ["cd", "true", "export", "set", "sleep"];
const FILTERS: [&str; 15] = ["cat", "head", "tail", "rg", "grep", "sed", "ls", "wc", "cut", "jq", "sort", "uniq", "xargs", "echo", "printf"];
const LOOPS: [&str; 5] = ["until", "while", "for", "do", "done"];
const SEPARATORS: [&str; 5] = ["&&", "||", ";", "|", "\n"];

pub fn command_of(text: &str) -> Option<&str> {
    let fields = text.split_once(' ').filter(|(tool, _)| !tool.contains('=')).map_or(text, |(_, rest)| rest);
    let start = ["command=", "cmd="].iter().find_map(|key| fields.strip_prefix(key))?;
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
        if words.first() == Some(&"timeout") {
            words = after_timeout(&words[1..]);
        }
        if words.len() == before {
            return words;
        }
    }
}

fn after_timeout<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    while let Some(word) = words.first().filter(|word| word.starts_with('-')) {
        words = &words[if TIMEOUT_VALUED.contains(word) || ["--signal", "--kill-after"].contains(word) { 2.min(words.len()) } else { 1 }..];
    }
    words.get(1..).unwrap_or(&[])
}

fn subcommand(word: &str) -> bool {
    word.bytes().next().is_some_and(|byte| byte.is_ascii_lowercase()) && word.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn filters(program: &str, arguments: &[&str]) -> bool {
    FILTERS.contains(&program) && (program != "sed" || (arguments.contains(&"-n") && !arguments.iter().any(|word| word.starts_with("-i"))))
}

fn identity(segment: &str) -> Option<String> {
    let words: Vec<&str> = segment.split_whitespace().collect();
    let words = unwrapped(&words);
    let program = words.first()?.rsplit('/').next()?.trim_start_matches('(');
    if program.is_empty() || INERT.contains(&program) || filters(program, &words[1..]) || !program.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)) {
        return None;
    }
    let verb = SUBCOMMANDED.contains(&program).then(|| words[1..].iter().find(|word| !word.starts_with('-')).filter(|word| subcommand(word))).flatten();
    Some(verb.map_or_else(|| program.to_owned(), |verb| format!("{program} {verb}")))
}

pub fn step(command: &str) -> Option<String> {
    let segments = segments(command);
    let opener = segments.first()?.split_whitespace().next()?;
    if LOOPS.contains(&opener) {
        return None;
    }
    segments.into_iter().find_map(identity)
}

pub fn anchors(step: &str) -> bool {
    let program = step.split(' ').next().unwrap_or(step);
    program == REMOTE || SUBCOMMANDED.contains(&program)
}
