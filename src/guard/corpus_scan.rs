use super::launch_critical::realpath;
use super::search_words::{self, Token, expand_home, is_file, join, normpath};
use crate::hook::{HookInput, Verdict};
use crate::shell;
use std::collections::HashMap;
use std::path::Path;

const UNIT: &str = "corpus-scan-guard";
const CHEAP_DEPTH: i64 = 3;
const GREP_LIKE: &[&str] = &[
    "grep", "egrep", "fgrep", "rgrep", "zgrep", "zegrep", "zfgrep",
];
const RG_LIKE: &[&str] = &["rg", "ripgrep", "ag", "ack", "ack-grep", "fd", "fdfind"];
const WRAPPERS: &[&str] = &[
    "sudo", "doas", "env", "nice", "ionice", "time", "nohup", "command", "builtin", "xargs",
    "timeout", "stdbuf", "setsid",
];
const WRAPPER_VALUED: &[&str] = &[
    "-n",
    "-u",
    "-c",
    "-e",
    "-I",
    "-P",
    "-L",
    "--max-procs",
    "--replace",
    "--adjustment",
    "-k",
    "-s",
];
const VALUED: &[&str] = &[
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
    "-r",
    "--replace",
    "--max-depth",
    "--maxdepth",
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
const GREP_NOT_VALUED: &[&str] = &["-r", "-R", "-E", "-d", "-D"];
const DEPTH_OPTIONS: &[&str] = &["--max-depth", "--maxdepth", "-maxdepth", "--depth", "-d"];

pub fn evaluate(input: &HookInput) -> Verdict {
    let Some((command, cwd)) = search_words::bash_call(input) else {
        return Verdict::Allow;
    };
    let home = std::env::var("HOME").unwrap_or_default();
    match refusal(command, &cwd, &home) {
        Some(reason) if !crate::hook::authoring_guards_off(UNIT) => Verdict::Deny(reason),
        _ => Verdict::Allow,
    }
}

enum Seg {
    Lit(String),
    Wild,
}

impl Seg {
    fn is(&self, name: &str) -> bool {
        match self {
            Seg::Wild => true,
            Seg::Lit(text) => text == name,
        }
    }
}

fn globbed(text: &str) -> bool {
    text.contains(['*', '?', '['])
}

fn expensive(segments: &[Seg]) -> bool {
    let Some(first) = segments.first() else {
        return true;
    };
    if first.is("archives") {
        return segments.len() == 1;
    }
    if !first.is("accounts") {
        return false;
    }
    if segments.len() <= 3 {
        return true;
    }
    let (container, rest) = (&segments[3], &segments[4..]);
    if container.is("projects") {
        return rest.is_empty();
    }
    container.is("sessions") && rest.len() <= 2
}

struct Corpus {
    home: String,
    roots: Vec<String>,
}

impl Corpus {
    fn new(home: &str) -> Self {
        let mut roots: Vec<String> = Vec::new();
        for base in [
            format!("{home}/code/north-data"),
            format!("{home}/.local/state/north"),
        ] {
            let real = realpath(Path::new(&base)).unwrap_or_else(|| base.clone());
            for root in [base, real] {
                if !roots.contains(&root) {
                    roots.push(root);
                }
            }
        }
        Self {
            home: home.to_owned(),
            roots,
        }
    }

    fn resolve(&self, token: &str, base: &str) -> String {
        normpath(&join(base, &expand_home(token, &self.home)))
    }

    fn segments(&self, path: &str) -> Option<Vec<Seg>> {
        let mut candidates = vec![path.to_owned()];
        if !globbed(path) {
            candidates.extend(realpath(Path::new(path)).filter(|real| real != path));
        }
        let tail = candidates.iter().find_map(|candidate| {
            self.roots.iter().find_map(|root| {
                if candidate == root {
                    return Some(String::new());
                }
                candidate
                    .strip_prefix(root.as_str())?
                    .strip_prefix('/')
                    .map(str::to_owned)
            })
        })?;
        Some(
            tail.split('/')
                .filter(|part| !part.is_empty())
                .map(|part| {
                    if globbed(part) {
                        Seg::Wild
                    } else {
                        Seg::Lit(part.to_owned())
                    }
                })
                .collect(),
        )
    }
}

struct Search {
    tool: String,
    arguments: Vec<String>,
    base: String,
}

fn run_length(text: &str) -> bool {
    let digits = text.trim_end_matches(['s', 'm', 'h', 'd']);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && text.len() - digits.len() <= 1
}

struct Scanner<'a> {
    corpus: &'a Corpus,
    here: String,
    tool: Option<String>,
    arguments: Vec<String>,
    found: Vec<Search>,
    at_command: bool,
    change_dir: bool,
    skip_next: bool,
}

impl Scanner<'_> {
    fn flush(&mut self) {
        if let Some(tool) = self.tool.take() {
            self.found.push(Search {
                tool,
                arguments: std::mem::take(&mut self.arguments),
                base: self.here.clone(),
            });
        }
        (self.at_command, self.change_dir, self.skip_next) = (true, false, false);
    }

    fn feed(&mut self, text: String, quoted: bool) {
        if self.skip_next {
            self.skip_next = false;
        } else if self.change_dir {
            self.here = self.corpus.resolve(&text, &self.here);
            (self.change_dir, self.at_command) = (false, false);
        } else if self.at_command {
            self.command_word(&text, quoted);
        } else if self.tool.is_some() {
            self.arguments.push(text);
        }
    }

    fn command_word(&mut self, text: &str, quoted: bool) {
        let name = shell::basename(text);
        if shell::assignment(text) || (WRAPPERS.contains(&name) && !quoted) {
            return;
        }
        if WRAPPER_VALUED.contains(&text) {
            self.skip_next = true;
        } else if name == "cd" && !quoted && !text.starts_with('-') && !run_length(text) {
            self.change_dir = true;
        } else if !(text.starts_with('-') || run_length(text)) {
            if !quoted && is_search(name) {
                self.tool = Some(name.to_owned());
            }
            self.at_command = false;
        }
    }
}

fn searches(tokens: Vec<Token>, corpus: &Corpus, start: &str) -> Vec<Search> {
    let mut scanner = Scanner {
        corpus,
        here: start.to_owned(),
        tool: None,
        arguments: Vec::new(),
        found: Vec::new(),
        at_command: true,
        change_dir: false,
        skip_next: false,
    };
    for (kind, text, quoted) in tokens {
        if kind == "sep" {
            scanner.flush();
        } else {
            scanner.feed(text, quoted);
        }
    }
    scanner.flush();
    scanner.found
}

fn is_search(name: &str) -> bool {
    GREP_LIKE.contains(&name) || RG_LIKE.contains(&name) || name == "find"
}

fn python_int(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    let well_formed = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.bytes().all(|b| b.is_ascii_digit() || b == b'_');
    if !well_formed {
        return None;
    }
    let value: i64 = digits.replace('_', "").parse().unwrap_or(i64::MAX);
    Some(if trimmed.starts_with('-') {
        -value
    } else {
        value
    })
}

fn depth_limit(arguments: &[String]) -> Option<i64> {
    let mut best: Option<i64> = None;
    for (index, token) in arguments.iter().enumerate() {
        let (name, value) = match token.split_once('=') {
            Some((name, value)) if token.starts_with('-') => (name, Some(value)),
            _ => (token.as_str(), arguments.get(index + 1).map(String::as_str)),
        };
        if !DEPTH_OPTIONS.contains(&name) {
            continue;
        }
        if let Some(depth) = value.and_then(python_int) {
            best = Some(best.map_or(depth, |best| best.min(depth)));
        }
    }
    best
}

fn operands(tool: &str, arguments: &[String]) -> Vec<String> {
    if tool == "find" {
        return arguments
            .iter()
            .take_while(|t| !t.starts_with('-') && *t != "(" && *t != "!")
            .cloned()
            .collect();
    }
    let grep = GREP_LIKE.contains(&tool);
    let mut words = Vec::new();
    let mut skip = false;
    let mut rest = arguments.iter();
    while let Some(token) = rest.next() {
        if skip {
            skip = false;
        } else if token == "--" {
            words.extend(rest.by_ref().cloned());
        } else if token.starts_with('-') && token != "-" {
            let valued = VALUED.contains(&token.as_str())
                && !(grep && GREP_NOT_VALUED.contains(&token.as_str()));
            skip = !token.contains('=') && valued;
        } else {
            words.push(token.clone());
        }
    }
    let explicit_pattern = arguments.iter().any(|t| {
        matches!(t.as_str(), "-e" | "--regexp" | "-f" | "--file")
            || t.starts_with("--regexp=")
            || t.starts_with("--file=")
    });
    if !explicit_pattern && !words.is_empty() {
        words.remove(0);
    }
    words
}

pub(super) fn refusal(command: &str, cwd: &str, home: &str) -> Option<String> {
    let corpus = Corpus::new(home);
    let tokens = shell::tokenize(
        &shell::strip_heredocs(command),
        ";&|\n(){}`",
        false,
        false,
        " \t",
    );
    let mut hit: Option<(String, String)> = None;
    let mut doubled = false;
    let mut seen: HashMap<String, String> = HashMap::new();
    for search in searches(tokens, &corpus, cwd) {
        if GREP_LIKE.contains(&search.tool.as_str())
            && !search_words::recursive_grep(&search.arguments)
        {
            continue;
        }
        if depth_limit(&search.arguments).is_some_and(|depth| depth <= CHEAP_DEPTH) {
            continue;
        }
        let mut paths = operands(&search.tool, &search.arguments);
        if paths.is_empty() {
            paths.push(".".into());
        }
        for path in paths {
            let resolved = corpus.resolve(&path, &search.base);
            let Some(segments) = corpus.segments(&resolved) else {
                continue;
            };
            if !globbed(&resolved) {
                let real = realpath(Path::new(&resolved)).unwrap_or_else(|| resolved.clone());
                if is_file(&real) {
                    continue;
                }
                if seen.get(&real).is_some_and(|earlier| *earlier != path) {
                    doubled = true;
                }
                seen.insert(real, path.clone());
            }
            if hit.is_none() && expensive(&segments) {
                hit = Some((resolved, search.tool.clone()));
            }
        }
    }
    let (root, tool) = hit?;
    Some(reason(&tool, &root, doubled))
}

fn reason(tool: &str, root: &str, doubled: bool) -> String {
    let twice = if doubled {
        "You also named the same tree twice: ~/.local/state/north is a SYMLINK to ~/code/north-data, so that scans every byte a second time. "
    } else {
        ""
    };
    format!(
        "BLOCKED: `{tool}` rooted at {root} is an unscoped sweep of the agent-transcript corpus — ~99 GB on disk, 77 GiB of JSONL, growing ~9.5 GiB/day. One such sweep measured 3.5 GB RSS and a quarter of a 24-core machine. {twice}Use the index instead:\n  convo <terms>            full-text across every provider and account\n  convo -x '<literal>'     exact phrase, ids, error strings\n  convo session <uuid>     locate one session's transcripts\nSame lookups in 0.3-0.4s at ~31 MB, and the index refreshes at query time so it is never stale. Raw search is still allowed once convo names the file: one transcript path, a single day or project directory and deeper, any non-transcript subtree of north-data, `find <root> -maxdepth 2`, `rg --max-depth 2`, or a non-recursive grep. Rare explicit override: `north config agents off corpus-scan-guard` (persistent, live), or a session LAUNCHED with AGENT_NO_AUTHORING_HOOKS=1."
    )
}
