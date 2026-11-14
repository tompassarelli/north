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
const HOSTS_HIDDEN: &str = super::session_scope::HOSTS_HIDDEN;
const REFUSED: u8 = 126;
const REFUSAL: &str = "north ssh-session: a worker command reaches no other host over ssh, scp or rsync: its sandbox reads no ssh key and hides the agent and master sockets. Run a command on a build host with `north capacity offload run --host HOST --base BASE --head HEAD --timeout SECONDS -- 'COMMAND'` (`north capacity offload hosts` lists the hosts), read or restart a north unit or refresh a listed copy on a host with `north runtime host-op`, land with `north finish` or `safe-push`, and reach the ledger with `north threads`.";

pub fn run(arguments: &[String]) -> u8 {
    if matches!(arguments, [word] if word == "-h" || word == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let Some((real, rest)) = arguments.split_first() else {
        eprintln!("{USAGE}");
        return 1;
    };
    if let Some(refusal) = refusal(rest, std::env::var_os(HOSTS_HIDDEN).is_some_and(|hidden| hidden == "1")) {
        eprintln!("{refusal}");
        return REFUSED;
    }
    let session = std::env::var(SESSION_ENV).ok().filter(|session| !session.is_empty());
    let words = session.map_or_else(|| rest.to_vec(), |session| scoped(rest, &session, &suffix()));
    let error = Command::new(real).args(words).exec();
    eprintln!("north ssh-session: cannot run {real}: {error}");
    127
}

/// What a command whose sandbox hides every other host is told in place of ssh's key error, naming the verbs that reach one, unless the call only asks ssh about itself (`-V`, `-G`).
fn refusal(arguments: &[String], hosts_hidden: bool) -> Option<&'static str> {
    (hosts_hidden && !arguments.iter().any(|word| word == "-V" || word == "-G")).then_some(REFUSAL)
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
    let Some((destination, at)) = command_start(arguments) else { return arguments.to_vec() };
    let (options, command) = arguments.split_at(at);
    let destination = arguments[destination].as_str();
    let remote = command.join(" ");
    let program = program(&remote);
    let protocol = PROTOCOLS.contains(&program.as_str()) || program.starts_with("git-") || destination.split_once('@').is_some_and(|(user, _)| user == "git");
    if protocol || program.is_empty() {
        return arguments.to_vec();
    }
    let quoted = format!("'{}'", remote.replace('\'', r"'\''"));
    let unit = scope_unit(session, suffix);
    let wrapped = format!(
        "q={quoted}; if [ -S \"$XDG_RUNTIME_DIR/bus\" ] && command -v systemd-run >/dev/null 2>&1; then exec systemd-run --user --scope --quiet --collect --expand-environment=no --unit={unit} -- \"${{SHELL:-/bin/sh}}\" -c \"$q\"; else exec \"${{SHELL:-/bin/sh}}\" -c \"$q\"; fi"
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

/// The destination's index and the remote command's first index, reading options on both sides of the destination as OpenSSH does, because nix's ssh stores pass theirs after it.
fn command_start(arguments: &[String]) -> Option<(usize, usize)> {
    let (mut at, mut destination) = (0, None);
    while let Some(word) = arguments.get(at) {
        if word == "--" {
            let destination = destination.unwrap_or(at + 1);
            let command = destination.max(at) + 1;
            return (command < arguments.len()).then_some((destination, command));
        }
        let Some(flags) = word.strip_prefix('-').filter(|flags| !flags.is_empty()) else {
            match destination {
                Some(destination) => return Some((destination, at)),
                None => destination = Some(at),
            }
            at += 1;
            continue;
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
