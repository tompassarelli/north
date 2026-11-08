use std::path::Path;

pub const IGNORED: [(&str, &str); 25] = [
    ("PWD", "the working directory is the checkout, whose location the path set records as a path inside the tree"),
    ("OLDPWD", "the previous directory is shell bookkeeping"),
    ("SHLVL", "shell nesting depth is bookkeeping"),
    ("_", "the shell sets it to the last program it ran"),
    ("HOSTNAME", "the host name differs between build hosts and a test that reads it reads an ambient file"),
    ("USER", "the account name never selects a test's files, and its home is an ambient path the recorder refuses"),
    ("LOGNAME", "the account name never selects a test's files, and its home is an ambient path the recorder refuses"),
    ("TMPDIR", "the sandbox gives each unit a fresh scratch directory"),
    ("PATH", "a program a test runs is read as a path, which the path set records or marks ambient"),
    ("TERM", "a check has no terminal"),
    ("COLORTERM", "a check has no terminal"),
    ("LS_COLORS", "a check has no terminal"),
    ("SAFE_PUSH_*", "the lander sets them per landing; a test that read one would read the checked tree through git, which makes it unreusable"),
    ("NORTH_LAND_*", "the lander sets them per landing"),
    ("AGENT_CAPACITY_*", "lease identities differ per run and size the run, not its verdict"),
    ("NEXTEST_*", "nextest sets run and slot identities per run"),
    ("CARGO_TARGET_*", "the runner hook and the target directory select where code runs, not what a test reads"),
    ("CARGO_BUILD_JOBS", "parallelism sizes the build, not what a test reads"),
    ("RUST_TEST_THREADS", "parallelism sizes the run, not what a test reads"),
    ("SCCACHE_*", "the compiler cache server and its port differ per slot and never change what a test reads"),
    ("CARGO_MAKEFLAGS", "the job server handles differ per run"),
    ("MAKEFLAGS", "the job server handles differ per run"),
    ("SSH_*", "the connection that started the run says nothing about its files"),
    ("XDG_RUNTIME_DIR", "a per-login runtime directory is an ambient path the recorder refuses"),
    ("NORTH_VERDICT_CACHE_DIR", "it locates the store that holds this fingerprint"),
];

fn ignored(name: &str) -> bool {
    IGNORED.iter().any(|(pattern, _)| pattern.strip_suffix('*').map_or(*pattern == name, |prefix| name.starts_with(prefix)))
}

pub fn normalized(environment: impl IntoIterator<Item = (String, String)>, roots: &[(&Path, &str)]) -> Vec<(String, String)> {
    let mut kept: Vec<(String, String)> = environment.into_iter().filter(|(name, _)| !ignored(name)).map(|(name, value)| (name, replace_roots(&value, roots))).collect();
    kept.sort();
    kept
}

pub fn replace_roots(value: &str, roots: &[(&Path, &str)]) -> String {
    let mut ordered: Vec<&(&Path, &str)> = roots.iter().collect();
    ordered.sort_by_key(|(path, _)| std::cmp::Reverse(path.as_os_str().len()));
    ordered.iter().fold(value.to_owned(), |text, (path, label)| match path.to_str().filter(|path| path.len() > 1) {
        Some(path) => text.replace(path, label),
        None => text,
    })
}
