//! What a proxy call would read to find a cause: a trace, a journal or a process table on any
//! host, a file under a repository checkout, or another host past the fleet's status verbs.
use crate::guard::paths::{expanduser, join, normpath};
use north_base::shell::{basename, command, heredoc_bodies, is_separator, shell_substitutions, strip_heredocs, words};
use crate::platform::pyjson::Value;

const MAX_DEPTH: usize = 3;
const PREFIXES: &[&str] = &["env", "exec", "time", "nice", "ionice", "timeout", "builtin", "nohup", "setsid", "stdbuf"];
const TRACERS: &[&str] = &[
    "strace", "ltrace", "perf", "gdb", "lldb", "bpftrace", "bpftool", "trace-cmd", "valgrind", "journalctl", "dmesg", "coredumpctl", "ps", "pgrep", "pidof", "pstree", "top", "htop", "btop", "atop", "lsof", "fuser",
];
const READERS: &[&str] = &[
    "rg", "grep", "egrep", "fgrep", "ag", "cat", "bat", "less", "more", "head", "tail", "sed", "awk", "gawk", "cut", "nl", "wc", "sort", "uniq", "diff", "cmp", "xxd", "od", "hexdump", "strings", "file", "stat", "jq", "yq", "fd", "find", "ls", "eza", "tree", "du", "readlink", "realpath",
];
const SEARCHERS: &[&str] = &["rg", "ag", "fd", "find", "ls", "eza", "tree", "du"];
const GIT_READS: &[&str] = &["show", "diff", "grep", "blame", "cat-file", "log", "ls-files", "ls-tree", "whatchanged"];
const GIT_PATCHES: &[&str] = &["-p", "-u", "--patch", "--stat", "--numstat", "-L", "-S", "-G"];
const GIT_VALUED: &[&str] = &["-C", "-c", "--git-dir", "--work-tree"];
const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash"];
const WRAPPERS: &[&str] = &["north", "nix", "depbox"];
const SSH_VALUED: &[&str] = &["-B", "-b", "-c", "-D", "-E", "-e", "-F", "-I", "-i", "-J", "-L", "-l", "-m", "-O", "-o", "-p", "-Q", "-R", "-S", "-W", "-w"];
const COPIERS: &[&str] = &["scp", "rsync"];
const ROUTING: &[&str] = &["fleet", "threads", "gate"];
const GLUE: &[&str] = &["cd", "export", "echo", "printf", "date", "true", "head", "tail", "cut", "sort", "uniq", "wc", "tr", "column", "rg", "grep", "jq", "sed", "awk"];

pub struct Look<'a> {
    pub home: &'a str,
    pub cwd: &'a str,
    pub status_verb: &'a dyn Fn(&[&str]) -> bool,
}

pub fn diagnosis(look: &Look, tool: &str, input: &Value) -> Option<String> {
    let text = |key: &str| input[key].as_str().filter(|text| !text.is_empty());
    match tool {
        "Read" | "NotebookRead" => text("file_path").or_else(|| text("notebook_path")).map(|path| look.resolve(look.cwd, path)).filter(|path| in_checkout(path)).map(|path| format!("read {path}")),
        "Grep" | "Glob" => Some(text("path").map_or_else(|| look.cwd.to_owned(), |path| look.resolve(look.cwd, path))).filter(|path| in_checkout(path)).map(|path| format!("search {path}")),
        _ => text("command").or_else(|| text("cmd")).and_then(|command| look.shell(command, look.cwd, 0)),
    }
}

struct Stage<'s> {
    program: &'s str,
    args: &'s [String],
    piped: bool,
}

impl Look<'_> {
    fn resolve(&self, cwd: &str, word: &str) -> String {
        normpath(&join(cwd, &expanduser(word, self.home)))
    }

    fn shell(&self, text: &str, cwd: &str, depth: usize) -> Option<String> {
        if depth > MAX_DEPTH {
            return None;
        }
        let (text, run) = data_blanked(text);
        let text = &text;
        if let Some(found) = run.iter().find_map(|body| self.shell(body, cwd, depth + 1)) {
            return Some(found);
        }
        if let Some(found) = shell_substitutions(text).iter().find_map(|body| self.shell(body, cwd, depth + 1)) {
            return Some(found);
        }
        let tokens = words(text).unwrap_or_else(|| text.split_whitespace().map(str::to_owned).collect());
        let mut cwd = cwd.to_owned();
        let mut piped = false;
        for (segment, separator) in split(&tokens) {
            let start = command(segment, PREFIXES);
            if let Some((program, args)) = segment.get(start..).and_then(<[String]>::split_first) {
                let stage = Stage { program: basename(program), args, piped };
                if let Some(found) = self.stage(&stage, &cwd, depth) {
                    return Some(found);
                }
                if let ("cd" | "pushd", [dir]) = (stage.program, args) {
                    cwd = self.resolve(&cwd, dir);
                }
            }
            piped = matches!(separator, Some("|" | "|&"));
        }
        None
    }

    fn stage(&self, stage: &Stage, cwd: &str, depth: usize) -> Option<String> {
        let Stage { program, args, .. } = *stage;
        if let Some(found) = process_read(program, args) {
            return Some(found);
        }
        match program {
            "ssh" => self.ssh(args, depth),
            _ if COPIERS.contains(&program) => args.iter().find(|arg| remote_path(arg)).map(|arg| format!("copy {arg} from another host with {program}")),
            "git" => self.git(args, cwd),
            _ if SHELLS.contains(&program) => script(args).and_then(|script| self.shell(script, cwd, depth + 1)),
            _ if WRAPPERS.contains(&program) => tail(args).and_then(|rest| self.shell(&rest, self.home, depth + 1)),
            _ if READERS.contains(&program) => self.file_read(stage, cwd),
            _ => None,
        }
    }

    fn file_read(&self, stage: &Stage, cwd: &str) -> Option<String> {
        let Stage { program, args, piped } = *stage;
        if program == "sed" && args.iter().any(|arg| arg.starts_with("-i") || arg.starts_with("--in-place")) {
            return None;
        }
        let operands: Vec<&String> = args.iter().filter(|arg| !arg.starts_with('-') && path_like(arg)).filter(|arg| !piped || arg.starts_with(['/', '~'])).collect();
        if let Some(path) = operands.iter().map(|arg| self.resolve(cwd, arg)).find(|path| in_checkout(path)) {
            return Some(format!("read {path} with {program}"));
        }
        let searches_cwd = SEARCHERS.contains(&program) || (matches!(program, "grep" | "egrep" | "fgrep") && args.iter().any(|arg| matches!(arg.as_str(), "-r" | "-R" | "--recursive")));
        (searches_cwd && operands.is_empty() && !piped && in_checkout(cwd)).then(|| format!("search {cwd} with {program}"))
    }

    fn git(&self, args: &[String], cwd: &str) -> Option<String> {
        let mut dir = cwd.to_owned();
        let mut rest = args.iter();
        let verb = loop {
            let word = rest.next()?;
            match word.as_str() {
                "-C" => dir = self.resolve(&dir, rest.next()?),
                word if GIT_VALUED.contains(&word) => {
                    rest.next();
                }
                word if word.starts_with('-') => {}
                word => break word,
            }
        };
        let patched = rest.any(|arg| GIT_PATCHES.iter().any(|flag| arg == flag || (flag.len() == 2 && arg.starts_with(flag))));
        let read = GIT_READS.contains(&verb) && (verb != "log" || patched);
        (read && in_checkout(&dir)).then(|| format!("run git {verb} in {dir}"))
    }

    fn ssh(&self, args: &[String], depth: usize) -> Option<String> {
        let mut rest = args.iter();
        let host = loop {
            let word = rest.next()?;
            match word.as_str() {
                word if SSH_VALUED.contains(&word) => {
                    rest.next();
                }
                word if word.starts_with('-') => {}
                word => break word,
            }
        };
        let remote: Vec<&str> = rest.map(String::as_str).skip_while(|word| *word == "--").collect();
        if remote.is_empty() {
            return Some(format!("open a shell on {host} over ssh"));
        }
        let remote = remote.join(" ");
        if let Some(found) = self.shell(&remote, self.home, depth + 1) {
            return Some(format!("{found} on {host} over ssh"));
        }
        (!self.status_only(&remote)).then(|| format!("run `{remote}` on {host} over ssh, past the fleet, threads, gate and status verbs"))
    }

    fn status_only(&self, text: &str) -> bool {
        let Some(tokens) = words(text) else { return false };
        split(&tokens).into_iter().all(|(segment, _)| {
            let start = command(segment, PREFIXES);
            let Some((program, args)) = segment.get(start..).and_then(<[String]>::split_first) else { return true };
            let program = basename(program);
            let words: Vec<&str> = args.iter().map(String::as_str).filter(|word| !word.starts_with('-')).collect();
            match program {
                "north" => words.first().is_some_and(|verb| ROUTING.contains(verb)) || (self.status_verb)(&words),
                "cd" | "export" => true,
                _ => GLUE.contains(&program) && !args.iter().any(|arg| arg.starts_with(['/', '~', '.'])),
            }
        })
    }
}

fn data_blanked(text: &str) -> (String, Vec<String>) {
    let feeds_a_program = |line: &str| line.split_once("<<").is_some_and(|(head, tail)| tail.contains('|') || words(head).is_some_and(|words| words.iter().any(|word| SHELLS.contains(&basename(word)) || matches!(basename(word), "ssh" | "eval" | "source" | "xargs"))));
    let run = heredoc_bodies(text).into_iter().filter(|body| text[..body.start].trim_end_matches('\n').rsplit('\n').next().is_some_and(feeds_a_program)).map(|body| text[body].to_owned()).collect();
    (strip_heredocs(text), run)
}

fn process_read(program: &str, args: &[String]) -> Option<String> {
    if TRACERS.contains(&program) {
        return Some(format!("run {program}, a trace, journal or process-table read"));
    }
    if program == "systemctl" && args.iter().any(|arg| arg == "status") {
        return Some("read a unit's journal with systemctl status".to_owned());
    }
    args.iter().any(|arg| arg.starts_with("/proc/") || arg == "/proc").then(|| format!("read the process table under /proc with {program}"))
}

fn split(tokens: &[String]) -> Vec<(&[String], Option<&str>)> {
    let mut out = Vec::new();
    let mut start = 0;
    for (at, token) in tokens.iter().enumerate() {
        if is_separator(token) {
            out.push((&tokens[start..at], Some(token.as_str())));
            start = at + 1;
        }
    }
    out.push((&tokens[start..], None));
    out
}

fn script(args: &[String]) -> Option<&str> {
    let at = args.iter().position(|arg| arg == "-c" || (arg.starts_with('-') && !arg.starts_with("--") && arg.ends_with('c')))?;
    args.get(at + 1).map(String::as_str)
}

fn tail(args: &[String]) -> Option<String> {
    let at = args.iter().position(|arg| matches!(arg.as_str(), "--" | "-c" | "--command"))?;
    let rest = &args[at + 1..];
    (!rest.is_empty()).then(|| rest.join(" "))
}

fn path_like(word: &str) -> bool {
    word == "." || word == ".." || word.contains('/') || word.starts_with('~') || word.rsplit_once('.').is_some_and(|(stem, extension)| !stem.is_empty() && (1..=5).contains(&extension.len()) && extension.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn remote_path(word: &str) -> bool {
    !word.starts_with('-') && !word.contains("://") && word.split_once(':').is_some_and(|(host, _)| !host.is_empty() && !host.contains('/') && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '@')))
}

fn in_checkout(path: &str) -> bool {
    path.contains("/code/") || path.ends_with("/code")
}
