mod delete;
mod flow;
mod heredoc;
mod log;
mod paths;
mod publish;
mod safe_push;
mod transfer;

use crate::hook::{HookInput, Verdict};
use crate::shell::{tripwire_tokens, tripwire_word};
use regex::Regex;
use std::collections::BTreeMap;

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

type Hold = dyn Fn(std::time::Instant, &str, &str, &[String]) -> Option<String>;
const OVERRIDE: &str = "deliberate path: `north config agents off north-hook`, run it, `north config agents on north-hook`";
const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash", "ksh", "source", ".", "eval"];
static VARIABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(:\?[^}]*)?\}|\$([A-Za-z_][A-Za-z0-9_]*)").unwrap()
});
static ASSIGNMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*=").unwrap());
static MUTATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(createCommitOnBranch|createRef|updateRefs?)([^A-Za-z]|$)").unwrap()
});
static GIT_DATA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|/)repos/[^/]+/[^/]+/(git/(refs|commits|trees|blobs|tags)|contents)(/|$|[?])")
        .unwrap()
});

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}
fn base(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or("")
}
fn strip(s: &str) -> &str {
    let s = s.strip_prefix('"').unwrap_or(s);
    let s = s.strip_suffix('"').unwrap_or(s);
    let s = s.strip_prefix('\'').unwrap_or(s);
    let s = s.strip_suffix('\'').unwrap_or(s);
    s.strip_suffix(')').unwrap_or(s)
}
fn under(p: &str, root: &str) -> bool {
    p == root || p.strip_prefix(root).is_some_and(|s| s.starts_with('/'))
}
fn lexical(p: &str) -> String {
    let mut out = PathBuf::new();
    for c in Path::new(p).components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            _ => out.push(c.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}
fn git(dir: &str, args: &[&str]) -> Option<String> {
    let result = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    result.status.success().then(|| {
        String::from_utf8_lossy(&result.stdout)
            .trim_end_matches('\n')
            .to_owned()
    })
}
fn git_root(p: &str) -> Option<String> {
    let path = Path::new(p);
    let dir = if path.is_dir() { path } else { path.parent()? };
    git(&dir.to_string_lossy(), &["rev-parse", "--show-toplevel"])
}
fn sep(s: &str) -> bool {
    matches!(s, ";" | "|" | "&&" | "||")
}
fn redirect(s: &str) -> Option<bool> {
    (s.contains('<') || s.contains('>'))
        .then(|| s.chars().all(|c| c.is_ascii_digit() || "<>&-".contains(c)))
}

struct State<'a> {
    input: &'a HookInput,
    home: String,
    cache: String,
    data: String,
    tmp: String,
    cwd: String,
    safe_push: bool,
    vars: BTreeMap<String, (String, usize)>,
    cwds: Vec<String>,
    scopes: Vec<Vec<String>>,
    cd_dirs: Vec<String>,
    created: Vec<String>,
    tokens: Vec<String>,
    previous: String,
    unread: String,
    delete_seen: bool,
    secret_hit: bool,
    localhost: bool,
    hold: &'a Hold,
    deadline: std::time::Instant,
}
impl<'a> State<'a> {
    fn new(input: &'a HookInput, get: &impl Fn(&str) -> String, hold: &'a Hold) -> Self {
        let home = get("HOME");
        let fallback = |key: &str, value: String| {
            let got = get(key);
            if got.is_empty() { value } else { got }
        };
        let cmd = input.command().and_then(|c| c.as_str()).unwrap_or("");
        let tokens = tripwire_tokens(&heredoc::strip_data_bodies(cmd));
        let mut prev = "";
        let mut secret_hit = false;
        for token in &tokens {
            if prev == "-i" || prev == "--identity" {
                prev = token;
                continue;
            }
            let s = strip(token);
            secret_hit |= transfer::secret(s);
            prev = s;
        }
        Self {
            input,
            cache: fallback("XDG_CACHE_HOME", format!("{home}/.cache")),
            data: fallback("XDG_DATA_HOME", format!("{home}/.local/share")),
            tmp: get("TMPDIR"),
            cwd: input
                .cwd()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .unwrap_or_default()
                        .display()
                        .to_string()
                }),
            safe_push: !get("SAFE_PUSH_ACTIVE").is_empty(),
            vars: BTreeMap::from([("HOME".into(), (home.clone(), 0))]),
            home,
            cwds: vec![String::new()],
            scopes: Vec::new(),
            cd_dirs: Vec::new(),
            created: Vec::new(),
            tokens,
            previous: String::new(),
            unread: String::new(),
            delete_seen: false,
            secret_hit,
            localhost: ["localhost", "127.0.0.1", "::1"]
                .iter()
                .any(|s| cmd.contains(s)),
            hold,
            deadline: crate::autoland_status::hook_deadline(),
        }
    }
    fn expand(&self, text: &str) -> String {
        VARIABLE
            .replace_all(text, |c: &regex::Captures<'_>| {
                let name = c.get(1).or_else(|| c.get(3)).unwrap().as_str();
                self.vars
                    .get(name)
                    .map(|v| v.0.as_str())
                    .unwrap_or(c.get(0).unwrap().as_str())
                    .to_owned()
            })
            .into_owned()
    }
    fn tilde(&self, text: &str) -> String {
        if text == "~" {
            self.home.clone()
        } else if let Some(tail) = text.strip_prefix("~/") {
            format!("{}/{tail}", self.home)
        } else {
            text.to_owned()
        }
    }
    fn resolve(&self, text: &str, cwd: &str) -> Option<String> {
        let t = self.tilde(&self.expand(strip(text)));
        if t.starts_with('~') {
            return None;
        }
        let t = if t.starts_with('/') {
            t
        } else {
            format!("{}/{t}", if cwd.is_empty() { &self.cwd } else { cwd })
        };
        let t = t.split(['*', '?']).next().unwrap_or("");
        if t.is_empty() || t.contains(['$', '`', '\x02']) {
            None
        } else {
            Some(lexical(t))
        }
    }
    fn open_scopes(&mut self, start: usize, end: usize) -> usize {
        let mut pops = 0;
        for k in start..end {
            let t = self.tokens[k].as_str();
            let opens = ["if", "while", "until", "for", "case", "select", "{"].contains(&t)
                || t.contains('(');
            if !t.contains("\\(") && !t.ends_with("\\)") && opens {
                self.scopes.push(self.cwds.clone());
            }
            if !t.ends_with("\\)") && (["fi", "done", "esac", "}"].contains(&t) || t.ends_with(')'))
            {
                pops += 1;
            }
        }
        pops
    }
    fn close_scopes(&mut self, pops: usize) {
        for _ in 0..pops {
            if let Some(saved) = self.scopes.pop() {
                for c in saved {
                    if !self.cwds.contains(&c) {
                        self.cwds.push(c);
                    }
                }
                self.vars
                    .retain(|_, (_, depth)| *depth <= self.scopes.len());
            }
        }
    }
    fn command(&mut self, mut i: usize, end: usize, next: &str) -> Result<(), String> {
        while i < end {
            if ["for", "select"].contains(&self.tokens[i].as_str())
                && let Some(name) = self.tokens.get(i + 1) {
                    self.vars.remove(name);
                }
            let word = tripwire_word(&self.tokens[i]).to_owned();
            if word == "_skip_user" {
                i += 2;
                continue;
            }
            if word.is_empty() {
                i += 1;
                continue;
            }
            let args = self.tokens[i + 1..end].to_vec();
            if ["cd", "pushd"].contains(&word.as_str()) {
                self.cd(&args, next);
                return Ok(());
            }
            return self.dispatch(&word, &args, i);
        }
        Ok(())
    }
    fn segment(&mut self, i: usize) -> Result<usize, String> {
        let end = (i..self.tokens.len())
            .find(|&j| sep(&self.tokens[j]))
            .unwrap_or(self.tokens.len());
        let next = self.tokens.get(end).cloned().unwrap_or_default();
        let pops = self.open_scopes(i, end);
        if !self.assignments(i, end, &next) {
            self.command(i, end, &next)?;
        }
        self.close_scopes(pops);
        Ok(end)
    }
    fn walk(&mut self) -> Result<(), String> {
        let mut i = 0;
        while i < self.tokens.len() {
            if sep(&self.tokens[i]) {
                self.previous = self.tokens[i].clone();
                i += 1;
                continue;
            }
            i = self.segment(i)?;
        }
        if self.input.data["hook_event_name"] == "PermissionRequest"
            && self.delete_seen
            && !self.unread.is_empty()
        {
            return Err(format!(
                "recursive delete target '{}' cannot be read before it runs. Delete the literal path instead: run the expansion on its own first, then rm -rf the path it printed",
                self.unread
            ));
        }
        Ok(())
    }
}

const PUSH_NOTE: &str = "tripwire: rewrote a raw `git push` to the equivalent `safe-push`, which gitleaks-scans the outgoing commits first. Write safe-push directly next time.";

fn pushed(input: &HookInput, get: &impl Fn(&str) -> String, hold: &Hold) -> Option<crate::platform::pyjson::Value> {
    if input.data["tool_name"] != "Bash" {
        return None;
    }
    let command = input.tool_input["command"].as_str()?;
    let cwd = input.cwd().unwrap_or(".");
    let candidate = safe_push::rewrite(command, cwd)?;
    let mut updated = input.tool_input.clone();
    updated["command"] = candidate.as_str().into();
    let mut data = input.data.clone();
    data["tool_input"] = updated.clone();
    let rewritten = HookInput::parse(crate::platform::pyjson::dumps(&data).as_bytes())?;
    matches!(decide(&rewritten, get, hold), Verdict::Allow).then_some(updated)
}

pub fn check(input: &HookInput) -> Verdict {
    decide(input, &env, &crate::autoland_status::origin_branches_hold)
}

fn decide(input: &HookInput, get: &impl Fn(&str) -> String, hold: &Hold) -> Verdict {
    let present = |key: &str| Some(get(key)).filter(|s| !s.is_empty());
    if input
        .command()
        .and_then(|c| c.as_str())
        .is_none_or(str::is_empty)
        || crate::hook::guards_off_in("north-hook", &present)
    {
        return Verdict::Allow;
    }
    let mut state = State::new(input, get, hold);
    match state.walk() {
        Err(reason) if reason.starts_with("raw 'git push'") && let Some(updated) = pushed(input, get, hold) => {
            Verdict::Rewrite(updated, PUSH_NOTE.into())
        }
        Err(reason) => {
            state.log(&reason, get);
            Verdict::Deny(format!("tripwire: {reason}"))
        }
        Ok(()) if state.delete_seen && input.data["hook_event_name"] == "PermissionRequest" => {
            Verdict::Approve
        }
        Ok(()) => Verdict::Allow,
    }
}

#[cfg(test)]
#[path = "tripwire_tests.rs"]
mod tests;
