use std::os::unix::process::CommandExt;
use std::process::Command;

const USAGE: &str = "usage: north ssh-session REAL_SSH [ARGS...]

The ssh a fleet session runs through the bin/ssh shim: a remote command runs on the far host inside
the user scope north-ssh-SESSION-XXXXXXXX.scope, so that host's capacity watchdog names the session
behind its load and can cap that work alone. The far host runs the command unscoped when it has no
user manager bus or no systemd-run. Logins without a command, -N, -O, -s, -G and -V,
git transport (a git user or a git-* command) and north, threads and nix protocol commands, which a
peer's forced-command key parses, pass through unchanged, as does every call outside a fleet session
(no NORTH_FLEET_SESSION).";

const SESSION_ENV: &str = "NORTH_FLEET_SESSION";
const SCOPE_PREFIX: &str = "north-ssh-";
const SUFFIX_LENGTH: usize = 8;
const VALUED: &[u8] = b"BbcDEeFIiJLlmOoPpQRSWw";
const UNSCOPED: &[u8] = b"NOsGV";
const PROTOCOLS: [&str; 4] = ["north", "threads", "nix-store", "nix-daemon"];

pub fn run(arguments: &[String]) -> u8 {
    if matches!(arguments, [word] if word == "-h" || word == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let Some((real, rest)) = arguments.split_first() else {
        eprintln!("{USAGE}");
        return 1;
    };
    let session = std::env::var(SESSION_ENV).ok().filter(|session| !session.is_empty());
    let words = session.map_or_else(|| rest.to_vec(), |session| scoped(rest, &session, &suffix()));
    let error = Command::new(real).args(words).exec();
    eprintln!("north ssh-session: cannot run {real}: {error}");
    127
}

fn suffix() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.subsec_nanos());
    format!("{:08x}", nanos ^ std::process::id().rotate_left(16))
}

pub fn scope_session(unit: &str) -> Option<&str> {
    let (name, suffix) = unit.strip_prefix(SCOPE_PREFIX)?.strip_suffix(".scope")?.rsplit_once('-')?;
    (!name.is_empty() && suffix.len() == SUFFIX_LENGTH && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(name)
}

fn scope_unit(session: &str, suffix: &str) -> String {
    let name: String = session.chars().map(|character| if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') { character } else { '_' }).collect();
    format!("{SCOPE_PREFIX}{name}-{suffix}.scope")
}

fn scoped(arguments: &[String], session: &str, suffix: &str) -> Vec<String> {
    let Some(at) = command_start(arguments) else { return arguments.to_vec() };
    let (options, command) = arguments.split_at(at);
    let destination = options.last().map_or("", String::as_str);
    let command = command.strip_prefix(&["--".to_owned()]).unwrap_or(command);
    let remote = command.join(" ");
    let program = program(&remote);
    let protocol = PROTOCOLS.contains(&program.as_str()) || program.starts_with("git-") || destination.split_once('@').is_some_and(|(user, _)| user == "git");
    if protocol || program.is_empty() {
        return arguments.to_vec();
    }
    let quoted = format!("'{}'", remote.replace('\'', r"'\''"));
    let unit = scope_unit(session, suffix);
    let wrapped = format!(
        "if [ -S \"$XDG_RUNTIME_DIR/bus\" ] && command -v systemd-run >/dev/null 2>&1; then exec systemd-run --user --scope --quiet --collect --expand-environment=no --unit={unit} -- \"${{SHELL:-/bin/sh}}\" -c {quoted}; else exec \"${{SHELL:-/bin/sh}}\" -c {quoted}; fi"
    );
    options.iter().cloned().chain([wrapped]).collect()
}

/// The program a remote command runs, past an `env NAME=VALUE ...` prefix, because the threads client sends its ledger calls as `env THREADS_ACTOR=... north threads ...`.
fn program(remote: &str) -> String {
    let words = words(remote);
    let mut words = words.iter().map(String::as_str).peekable();
    if words.next_if_eq(&"env").is_some() {
        while words.next_if(|word| word.split_once('=').is_some_and(|(name, _)| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))).is_some() {}
    }
    words.next().map_or_else(String::new, |word| word.rsplit('/').next().unwrap_or_default().to_owned())
}

/// The words of a remote command whose only quoting is the single quotes `shell_quote` writes.
fn words(remote: &str) -> Vec<String> {
    let (mut words, mut word, mut quoted, mut started) = (Vec::new(), String::new(), false, false);
    for character in remote.chars() {
        match character {
            '\'' => (quoted, started) = (!quoted, true),
            ' ' | '\t' | '\n' if !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                }
                started = false;
            }
            _ => {
                word.push(character);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

fn command_start(arguments: &[String]) -> Option<usize> {
    let mut at = 0;
    while let Some(word) = arguments.get(at) {
        if word == "--" {
            return (at + 2 < arguments.len()).then_some(at + 2);
        }
        let Some(flags) = word.strip_prefix('-').filter(|flags| !flags.is_empty()) else {
            return (at + 1 < arguments.len()).then_some(at + 1);
        };
        for (position, flag) in flags.bytes().enumerate() {
            if UNSCOPED.contains(&flag) {
                return None;
            }
            if VALUED.contains(&flag) {
                if position + 1 == flags.len() {
                    at += 1;
                }
                break;
            }
        }
        at += 1;
    }
    None
}

#[cfg(test)]
#[path = "ssh_session_tests.rs"]
mod tests;
