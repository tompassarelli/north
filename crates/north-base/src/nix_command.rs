//! The command a wrapper word runs: `env`, `bash -c` and `sh -c`, and every Nix form that runs an inner command or builds, read the same way by the heavy-command hook and the capacity gate.

use crate::shell;

const SUBCOMMANDS: &[&str] = &[
    "build", "bundle", "config", "copy", "daemon", "derivation", "develop", "edit", "env", "eval", "flake", "fmt", "formatter", "hash", "help", "help-stores", "key", "log", "nar",
    "path-info", "print-dev-env", "profile", "realisation", "registry", "repl", "run", "search", "shell", "store", "upgrade-nix", "why-depends",
];
const ONE_VALUE: &[&str] = &[
    "--extra-experimental-features", "--experimental-features", "--log-format", "--max-jobs", "-j", "--cores", "--store", "--eval-store", "-I", "--include", "--builders",
    "--substituters", "--extra-substituters", "--system", "--extra-platforms", "--inputs-from", "--reference-lock-file", "--output-lock-file", "--profile",
];
const TWO_VALUES: &[&str] = &["--option", "--override-input", "--arg", "--argstr", "--override-flake"];

#[derive(Debug, PartialEq, Eq)]
pub enum Inner<'a> {
    Command { words: Vec<String>, directories: Vec<&'a str> },
    Script(&'a str),
    NixBuild,
}

/// What `words` runs when its first word is a wrapper this module knows; `None` for any other command.
pub fn unwrap(words: &[String]) -> Option<Inner<'_>> {
    let (name, args) = words.split_first()?;
    match shell::basename(name) {
        "env" => Some(env(args)),
        "bash" | "sh" => {
            let marker = args.iter().position(|word| word.starts_with('-') && !word.starts_with("--") && word.contains('c'))?;
            args.get(marker + 1).map(|source| Inner::Script(source))
        }
        "nix" => nix(args),
        "nix-build" => Some(Inner::NixBuild),
        "nix-shell" => Some(args.iter().position(|word| word == "--run" || word == "--command").and_then(|marker| args.get(marker + 1)).map_or(Inner::NixBuild, |source| Inner::Script(source))),
        _ => None,
    }
}

const LAUNCHERS: &[&str] = &["bwrap", "firejail", "systemd-run", "sudo", "doas", "taskset", "chrt", "unshare", "setpriv", "nsenter", "xargs", "flock"];
const LAUNCHED: &[&str] = &["cargo", "bash", "sh", "bun", "bunx", "npx", "tsc", "nix", "nix-shell", "nix-build", "env", "machine-capacity", "north"];

/// The commands a launcher word runs, when its first word is one such as `bwrap`, `sudo` or `taskset`: its options take values this parser does not know, so each later word that names a program a gate classifies starts one candidate command.
pub fn launched(words: &[String]) -> Option<Vec<&[String]>> {
    LAUNCHERS.contains(&shell::basename(words.first()?)).then(|| (1..words.len()).filter(|&i| LAUNCHED.contains(&shell::basename(&words[i]))).map(|i| &words[i..]).collect())
}

fn env(args: &[String]) -> Inner<'_> {
    let mut directories = Vec::new();
    let mut i = 0;
    while let Some(word) = args.get(i) {
        match word.as_str() {
            "-u" | "--unset" => i += 2,
            "-C" | "--chdir" => {
                directories.extend(args.get(i + 1).map(String::as_str));
                i += 2;
            }
            _ if word.starts_with("--chdir=") => {
                directories.push(&word["--chdir=".len()..]);
                i += 1;
            }
            _ if word.starts_with('-') || word.contains('=') => i += 1,
            _ => break,
        }
    }
    let i = i.min(args.len());
    let assignments = args[..i].iter().filter(|word| shell::assignment(word)).cloned();
    Inner::Command { words: assignments.chain(args[i..].iter().cloned()).collect(), directories }
}

fn nix(args: &[String]) -> Option<Inner<'_>> {
    let (subcommand, rest) = subcommand(args)?;
    match subcommand {
        "develop" | "shell" => Some(rest.iter().position(|word| word == "-c" || word == "--command").map_or(Inner::NixBuild, |marker| Inner::Command { words: rest[marker + 1..].to_vec(), directories: Vec::new() })),
        "run" | "build" => Some(Inner::NixBuild),
        "flake" => rest.iter().find(|word| !word.starts_with('-')).is_some_and(|word| word == "check").then_some(Inner::NixBuild),
        _ => None,
    }
}

fn subcommand(args: &[String]) -> Option<(&str, &[String])> {
    let mut i = 0;
    while let Some(word) = args.get(i) {
        let word = word.as_str();
        if SUBCOMMANDS.contains(&word) {
            return Some((word, &args[i + 1..]));
        }
        i += if TWO_VALUES.contains(&word) {
            3
        } else if ONE_VALUE.contains(&word) {
            2
        } else {
            1
        };
    }
    None
}

/// `words` with `--cores CORES --max-jobs 1` after the program when it is a Nix build that sets neither, since the daemon builds outside the caller's lease scope and takes its parallelism only from these flags.
pub fn sized(words: &[String], cores: &str) -> Option<Vec<String>> {
    let (program, args) = words.split_first()?;
    let builds = matches!(shell::basename(program), "nix" | "nix-build" | "nix-shell") && unwrap(words) == Some(Inner::NixBuild);
    let set = args.iter().any(|word| matches!(word.as_str(), "--cores" | "--max-jobs" | "-j") || word.starts_with("-j") && word[2..].parse::<u32>().is_ok());
    (builds && !set).then(|| [program.clone(), "--cores".to_owned(), cores.to_owned(), "--max-jobs".to_owned(), "1".to_owned()].into_iter().chain(args.iter().cloned()).collect())
}
