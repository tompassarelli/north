use super::search_words::{self, Located, MAX_NESTING, expand_home, is_dir, join, normpath};
use crate::hook::{HookInput, Verdict};
use crate::shell;
use regex::Regex;
use std::sync::LazyLock;

const UNIT: &str = "resource-safe-search-guard";
const ALWAYS_RECURSIVE: &[&str] = &[
    "rg", "ripgrep", "ag", "ack", "ack-grep", "fd", "fdfind", "rgrep",
];
const GREP_LIKE: &[&str] = &["grep", "egrep", "fgrep", "zgrep", "zegrep", "zfgrep"];
const VALUED_OPTIONS: &[&str] = &[
    "-e",
    "--regexp",
    "-f",
    "--file",
    "-m",
    "--max-count",
    "-A",
    "--after-context",
    "-B",
    "--before-context",
    "-C",
    "--context",
    "-d",
    "--directories",
    "-D",
    "--devices",
    "--include",
    "--exclude",
    "--exclude-dir",
    "--exclude-from",
    "--binary-files",
    "--color",
    "--colour",
    "--label",
    "--group-separator",
    "-g",
    "--glob",
    "--iglob",
    "-t",
    "--type",
    "-T",
    "--type-not",
    "--replace",
    "--max-depth",
    "--max-filesize",
    "--sort",
    "--sortr",
    "-j",
    "--threads",
    "--pre",
    "--ignore-file",
    "-E",
    "--encoding",
    "--path-separator",
];
const INFORMATIONAL: &[&str] = &[
    "--help",
    "--version",
    "--type-list",
    "--pcre2-version",
    "--generate",
];
const VIRTUAL_ROOTS: &[&str] = &["/proc", "/sys", "/dev", "/run"];

static PROC_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^/proc/[^/]+/(?:cwd|root|exe)(?:/|\n?\z)|^/proc/[^/]+/(?:fd|map_files)/[^/]+(?:/|\n?\z)",
    )
    .unwrap()
});

struct Refusal {
    tool: String,
    target: String,
    shape: &'static str,
    repository: bool,
}

pub fn evaluate(input: &HookInput) -> Verdict {
    let Some((command, cwd)) = search_words::bash_call(input) else {
        return Verdict::Allow;
    };
    let home = std::env::var("HOME").unwrap_or_default();
    match refusal(command, &cwd, &home) {
        Some(reason) if !crate::guard::activation::authoring_guards_off(UNIT) => Verdict::Deny(reason),
        _ => Verdict::Allow,
    }
}

impl Refusal {
    fn reason(&self) -> String {
        let Self {
            tool,
            target,
            shape,
            ..
        } = self;
        if self.repository {
            return format!(
                "BLOCKED: `{tool}` would perform {shape} at {target}, traversing main and every worktree. Select one exact checkout or subtree first, then run the scoped search there (for example, `rg --files {target}/main`)."
            );
        }
        format!(
            "BLOCKED: `{tool}` would perform {shape} at {target}. Do not use recursive content search over /proc, /sys, /dev, or /run. Select one process with `ps -eo pid=,comm=,args=` (or `ps -p PID -o ...`) and query only the needed metadata, for example `readlink -e /proc/PID/cwd`. Use `cat` or `stat` for one known metadata file. If the resolved cwd contains the content of interest, run ordinary scoped `rg` on that filesystem path instead."
        )
    }
}

pub(super) fn refusal(command: &str, cwd: &str, home: &str) -> Option<String> {
    inspect_command(command, cwd, home, 0).map(|found| found.reason())
}

fn inspect_command(text: &str, base: &str, home: &str, depth: usize) -> Option<Refusal> {
    let tokens = shell::tokenize(
        &shell::strip_heredocs(text),
        ";&|\n()`",
        false,
        false,
        " \t\r",
    );
    search_words::segments(tokens)
        .into_iter()
        .find_map(|segment| inspect_segment(&segment.words, segment.piped, base, home, depth))
}

fn is_search(name: &str) -> bool {
    ALWAYS_RECURSIVE.contains(&name) || GREP_LIKE.contains(&name)
}

fn inspect_segment(
    words: &[String],
    piped: bool,
    base: &str,
    home: &str,
    depth: usize,
) -> Option<Refusal> {
    if words.is_empty() || depth > MAX_NESTING {
        return None;
    }
    let index = match search_words::locate(words, &["{", "}"], is_search) {
        Located::Nested(text) => return inspect_command(&text, base, home, depth + 1),
        Located::Tool(index) => index,
        Located::Other => return None,
    };
    let tool = shell::basename(&words[index]);
    let operands = path_operands(tool, &words[index + 1..], piped)?;
    let found = |target: String, shape, repository| Refusal {
        tool: tool.to_owned(),
        target,
        shape,
        repository,
    };
    if let Some((target, shape)) = dangerous_virtual_operand(&operands, base, home) {
        return Some(found(target, shape, false));
    }
    dangerous_repository_container(&operands, base, home)
        .map(|(target, shape)| found(target, shape, true))
}

#[derive(Default)]
struct Scan {
    words: Vec<String>,
    explicit_pattern: bool,
    files_mode: bool,
}

fn scan(arguments: &[String]) -> Scan {
    let mut found = Scan::default();
    let mut skip = false;
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        if skip {
            skip = false;
            continue;
        }
        if argument == "--" {
            found.words.extend(rest.by_ref().cloned());
            break;
        }
        let dashed = argument.starts_with('-');
        let option = if dashed {
            argument.split('=').next().unwrap_or("")
        } else {
            argument
        };
        found.explicit_pattern |= matches!(option, "-e" | "--regexp" | "-f" | "--file");
        found.files_mode |= option == "--files";
        if dashed && argument != "-" {
            skip = !argument.contains('=') && VALUED_OPTIONS.contains(&option);
        } else {
            found.words.push(argument.clone());
        }
    }
    found
}

fn reads_pattern_from_stdin(arguments: &[String]) -> bool {
    arguments.iter().enumerate().any(|(index, argument)| {
        (matches!(argument.as_str(), "-f" | "--file")
            && arguments.get(index + 1).is_some_and(|n| n == "-"))
            || argument == "--file=-"
    })
}

fn path_operands(tool: &str, arguments: &[String], piped_stdin: bool) -> Option<Vec<String>> {
    let informational = arguments
        .iter()
        .any(|a| INFORMATIONAL.contains(&a.split('=').next().unwrap_or("")));
    if informational || (GREP_LIKE.contains(&tool) && !search_words::recursive_grep(arguments)) {
        return None;
    }
    let Scan {
        mut words,
        explicit_pattern,
        files_mode,
    } = scan(arguments);
    if !explicit_pattern && !files_mode && !words.is_empty() {
        words.remove(0);
    }
    let explicit_stdin = !files_mode && words.iter().any(|w| w == "-");
    words.retain(|w| !explicit_stdin || w != "-");
    if !words.is_empty() {
        return Some(words);
    }
    let from_pipe = piped_stdin && !files_mode && !reads_pattern_from_stdin(arguments);
    Some(if explicit_stdin || from_pipe {
        Vec::new()
    } else {
        vec![".".into()]
    })
}

fn resolve_operand(operand: &str, base: &str, home: &str) -> String {
    let path = normpath(&join(base, &expand_home(operand, home)));
    for (alias, target) in [("/var/run", "/run"), ("/var/lock", "/run/lock")] {
        if let Some(tail) = path.strip_prefix(alias)
            && (tail.is_empty() || tail.starts_with('/'))
        {
            return format!("{target}{tail}");
        }
    }
    path
}

fn virtual_root(path: &str) -> Option<&'static str> {
    VIRTUAL_ROOTS.iter().copied().find(|root| {
        path == *root
            || path
                .strip_prefix(root)
                .is_some_and(|tail| tail.starts_with('/'))
    })
}

fn wildcard(text: &str) -> bool {
    text.contains(['*', '?', '[', '{'])
}

fn dangerous_virtual_operand(
    operands: &[String],
    base: &str,
    home: &str,
) -> Option<(String, &'static str)> {
    let virtual_operands: Vec<(&String, String, &str)> = operands
        .iter()
        .filter_map(|operand| {
            let path = resolve_operand(operand, base, home);
            virtual_root(&path).map(|root| (operand, path, root))
        })
        .collect();
    if virtual_operands.len() > 1 {
        return Some((
            virtual_operands[0].1.clone(),
            "fan-out across multiple virtual-root operands",
        ));
    }
    for (operand, path, root) in virtual_operands {
        if wildcard(operand) || wildcard(&path) {
            return Some((path, "shell-expanded or wildcard virtual-root traversal"));
        }
        if path == root {
            return Some((path, "recursive traversal from a virtual filesystem root"));
        }
        if PROC_LINK.is_match(&path) {
            return Some((
                path,
                "recursive traversal through process metadata symlinks",
            ));
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() || meta.file_type().is_symlink() {
            return Some((
                path,
                "recursive traversal through a virtual directory or symlink",
            ));
        }
        if !meta.is_file() {
            return Some((path, "content search over a virtual device or special file"));
        }
    }
    None
}

fn dangerous_repository_container(
    operands: &[String],
    base: &str,
    home: &str,
) -> Option<(String, &'static str)> {
    operands.iter().find_map(|operand| {
        let path = resolve_operand(operand, base, home);
        (is_dir(&join(&path, "main")) && is_dir(&join(&path, "worktrees")))
            .then_some((path, "recursive search rooted at a repository container"))
    })
}
