use super::CommitId;
use std::path::Path;

pub(super) const BESIDE_ENV: &str = "NORTH_OFFLOAD_BESIDE";
const HOMES: [&str; 3] = ["$HOME/", "${HOME}/", "~/"];
const CODE: &str = "code/";
const MAIN: &str = "/main";

/// A land check as the build host runs it: `command` reads each checkout beside the shipped one from the build host's copy pinned to `pins`, the `PROJECT=SHA` words naming the commit each is on here.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Beside {
    pub(super) command: String,
    pub(super) pins: String,
}

/// Rewrites each `$HOME/code/PROJECT/main` the command names, other than the shipped checkout `shipped`, to `$NORTH_OFFLOAD_BESIDE/PROJECT`, pinned to the commit `head_of` reads for it here; a checkout this host lacks stays as named.
pub(super) fn pin(command: &str, shipped: &str, home: &Path, head_of: impl Fn(&Path) -> Option<CommitId>) -> Beside {
    let mut rewritten = String::with_capacity(command.len());
    let mut pins: Vec<String> = Vec::new();
    let mut rest = command;
    while let Some((at, home_len, project)) = next_checkout(rest) {
        let relative = format!("{CODE}{project}{MAIN}");
        let named = home_len + relative.len();
        let head = (relative != shipped).then(|| head_of(&home.join(&relative))).flatten();
        rewritten.push_str(&rest[..at]);
        match head {
            Some(head) => {
                rewritten.push_str(&format!("${BESIDE_ENV}/{project}"));
                let word = format!("{project}={}", head.as_str());
                if !pins.contains(&word) {
                    pins.push(word);
                }
            }
            None => rewritten.push_str(&rest[at..at + named]),
        }
        rest = &rest[at + named..];
    }
    rewritten.push_str(rest);
    Beside { command: rewritten, pins: pins.join(" ") }
}

fn next_checkout(text: &str) -> Option<(usize, usize, &str)> {
    let mut from = 0;
    loop {
        let (at, home) = HOMES.iter().filter_map(|home| text[from..].find(home).map(|at| (from + at, home.len()))).min()?;
        let after = &text[at + home..];
        let project_len = after.strip_prefix(CODE).map_or(0, |tail| tail.find(|c: char| !(c.is_ascii_alphanumeric() || "._-".contains(c))).unwrap_or(tail.len()));
        let tail = &after[(CODE.len() + project_len).min(after.len())..];
        let ends = tail.strip_prefix(MAIN).is_some_and(|end| end.chars().next().is_none_or(|c| !(c.is_ascii_alphanumeric() || "._-".contains(c))));
        if project_len > 0 && ends {
            return Some((at, home, &after[CODE.len()..CODE.len() + project_len]));
        }
        from = at + home;
    }
}

