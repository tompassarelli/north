use crate::foundation::clock::Timestamp;
use crate::guard::paths;
use crate::hook::{HookInput, Verdict};
use crate::shell::{self, basename, shell_substitutions, strip_heredoc_bodies, unquoted_mask, whitespace};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::LazyLock;

const ID: &str = "north-hook";
const GENERIC_REASON: &str = "`main` is the clean checkout of a project — nothing is edited there, by anyone, and uncommitted state in it belongs to the human.";
const MUTATING_GIT: &[&str] = &[
    "commit",
    "add",
    "reset",
    "checkout",
    "switch",
    "restore",
    "merge",
    "rebase",
    "cherry-pick",
    "revert",
    "stash",
    "apply",
    "am",
    "push",
    "clean",
    "rm",
    "mv",
    "gc",
    "prune",
    "filter-branch",
    "update-ref",
];
const WRITE_COMMANDS: &[&str] = &["tee", "truncate", "install", "patch", "dd", "shred"];
const COPY_COMMANDS: &[&str] = &["cp", "mv", "rsync", "scp", "ln"];
const REMOTE_COPY_COMMANDS: &[&str] = &["rsync", "scp"];
const DESTRUCTIVE_COMMANDS: &[&str] = &["rm", "rmdir", "shred", "unlink"];
const READ_ONLY_COMMANDS: &[&str] = &[
    "grep", "egrep", "fgrep", "rg", "cat", "head", "tail", "less", "more", "wc", "ls", "echo",
    "printf", "file", "stat", "diff", "cmp", "cut", "tr", "jq", "nl", "basename", "dirname",
    "realpath", "readlink", "du", "which", "type", "man", "test", "[",
];
const INTERPRETERS: &[&str] = &[
    "python", "python3", "perl", "ruby", "node", "bb", "bash", "sh", "zsh",
];
const PACKAGE_MANAGERS: &[&str] = &[
    "bun", "bunx", "npm", "npx", "pnpm", "pnpx", "yarn", "deno", "cargo", "pip", "pip3", "uv",
    "poetry", "gem", "go", "nix",
];
const CONTROL_WORDS: &[&str] = &[
    "!", "{", "}", "(", ")", "if", "then", "elif", "else", "fi", "while", "until", "do", "done",
];
const BLOCK_OPEN: &[&str] = &["if", "while", "until", "for", "case", "select", "{"];
const BLOCK_CLOSE: &[&str] = &["fi", "done", "esac", "}"];
const PATCH_BEGIN: &str = "*** Begin Patch";
static VARIABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\$(?:\{([A-Za-z_][A-Za-z0-9_]*)\}|([A-Za-z_][A-Za-z0-9_]*))").unwrap()
});
static ASSIGNMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^([A-Za-z_][A-Za-z0-9_]*)=(.*)$").unwrap());
static PATCH_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\*\*\* (?:Add|Update|Delete) File:[\s\x1c-\x1f]+(.+?)[\s\x1c-\x1f]*$")
        .unwrap()
});
static PATCH_MOVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\*\*\* Move to:[\s\x1c-\x1f]+(.+?)[\s\x1c-\x1f]*$").unwrap()
});
static PATCH_ENVELOPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\*\*\* Begin Patch.*?(?:\*\*\* End Patch|\z)").unwrap());
static SEGMENT_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\s\x1c-\x1f]*(?:&&|\|\||[;|&\n])[\s\x1c-\x1f]*").unwrap());

type Env = BTreeMap<String, Option<String>>;

fn remote_spec(word: &str) -> bool {
    word.split_once(':')
        .is_some_and(|(host, _)| !host.is_empty() && !host.contains('/') && !host.starts_with('.'))
}

fn tokens(text: &str) -> Vec<String> {
    shell::shlex_split(text).unwrap_or_else(|| {
        text.split(whitespace)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    })
}
fn resolve(path: &str, cwd: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    paths::realpath(&Path::new(cwd).join(path))
}

#[derive(Clone)]
struct Hit {
    project: String,
    why: String,
    pin: Option<String>,
}
struct Policy {
    root: String,
    home: String,
}
impl Policy {
    fn from_env() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let named = std::env::var("LAUNCH_CRITICAL_CODE_ROOT")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{home}/code"));
        Self {
            root: paths::realpath(Path::new(&named)).unwrap_or(named),
            home,
        }
    }
    fn expanduser(&self, word: &str) -> String {
        paths::expanduser(word, &self.home)
    }
    fn parts(&self, path: &str) -> Option<Vec<String>> {
        if path.is_empty() {
            return None;
        }
        let real = paths::realpath(Path::new(path))?;
        let rest = if real == self.root {
            ""
        } else {
            real.strip_prefix(&format!("{}/", self.root))?
        };
        let parts: Vec<_> = rest
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if parts.is_empty() || parts[0] == "resources" {
            None
        } else {
            Some(parts)
        }
    }
    fn slot(&self, parts: &[String]) -> Option<(String, usize)> {
        for i in 1..parts.len() {
            let project = parts[..i].join("/");
            if Path::new(&format!("{}/{project}/main/.git", self.root)).exists() {
                return Some((project, i));
            }
        }
        None
    }
    fn protected(&self, path: &str) -> Option<Hit> {
        let parts = self.parts(path)?;
        if let Some((project, index)) = self.slot(&parts) {
            if parts[index] == "pins" && parts.len() > index + 1 {
                let leaf = &parts[index + 1];
                if leaf.ends_with(".pin") && parts.len() == index + 2 {
                    return None;
                }
                let name = leaf.strip_suffix(".pin").unwrap_or(leaf).to_owned();
                let manifest =
                    std::fs::File::open(format!("{}/{project}/pins/{name}.pin", self.root))
                        .ok()
                        .and_then(|file| {
                            let mut bytes = Vec::new();
                            file.take(16384).read_to_end(&mut bytes).ok()?;
                            let text: String = String::from_utf8_lossy(&bytes)
                                .replace("\r\n", "\n")
                                .replace('\r', "\n")
                                .chars()
                                .take(4096)
                                .collect();
                            let normalized = text
                                .split(whitespace)
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join(" ");
                            (!normalized.is_empty()).then_some(normalized)
                        });
                let listed = match manifest {
                    Some(text) => format!("Consumers, from {project}/pins/{name}.pin: {text}"),
                    None => format!(
                        "No {project}/pins/{name}.pin manifest exists — a pin whose consumers cannot be named is one the human has to rule on, not one an agent may edit."
                    ),
                };
                return Some(Hit {
                    why: format!(
                        "{project}/pins/{name} is an externally CONSUMED checkout. It is protected because something outside this repository reads it at exactly this path and revision — not because the dirt would be the human's. {listed}"
                    ),
                    project,
                    pin: Some(name),
                });
            }
            if parts[index] == "worktrees" {
                return None;
            }
        }
        for (i, part) in parts.iter().enumerate() {
            if i > 0 && ["worktrees", "pins"].contains(&part.as_str()) {
                break;
            }
            if part != "main" || i == 0 {
                continue;
            }
            if Path::new(&format!("{}/{}/.git", self.root, parts[..=i].join("/"))).exists() {
                let project = parts[..i].join("/");
                let why = match project.as_str() {
                    "north" => {
                        "north is launch-critical: its promoted binary is the normal `north` command, so uncommitted work here is absent from the operator runtime."
                    }
                    "nixos-config" => {
                        "nixos-config is launch-critical: it is the system source and `firn rebuild` snapshots commits, so uncommitted work here never reaches a generation."
                    }
                    _ => GENERIC_REASON,
                };
                return Some(Hit {
                    project,
                    why: why.into(),
                    pin: None,
                });
            }
        }
        None
    }
    fn spill(&self, path: &str) -> Option<String> {
        let parts = self.parts(path)?;
        let (project, index) = self.slot(&parts)?;
        (!["main", "worktrees", "pins"].contains(&parts[index].as_str())).then_some(project)
    }
    fn sidecar(&self, path: &str) -> Option<String> {
        let parts = self.parts(path)?;
        let (project, index) = self.slot(&parts)?;
        if parts.len() != index + 2 || parts[index] != "pins" {
            return None;
        }
        let name = parts[index + 1].strip_suffix(".pin")?;
        if name.len() != 40
            || !name
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return None;
        }
        let pin = format!("{}/{project}/pins/{name}", self.root);
        Path::new(&pin).is_dir().then_some(pin)
    }
    fn worktree_advice(&self, project: &str) -> String {
        let container = format!("{}/{project}", self.root);
        format!(
            "Work in a lane, then land through a ref:\n  mkdir -p {container}/worktrees\n  git -C {container}/main worktree add {container}/worktrees/SLUG -b SLUG\n  # edit + commit in {container}/worktrees/SLUG, then land it:\n  git -C {container}/main merge --ff-only SLUG\n  # (plain `git fetch LANE BRANCH:refs/heads/main` cannot be used: main is checked out)\n"
        )
    }
    fn advice(&self, hit: &Hit) -> String {
        let Some(name) = &hit.pin else {
            return self.worktree_advice(&hit.project);
        };
        let container = format!("{}/{}", self.root, hit.project);
        format!(
            "A pin is not a lane. Do not edit it, and do not cut a worktree from it:\n  cat {container}/pins/{name}.pin        # who consumes it, on what terms\nTo advance a consumer, leave this checkout untouched and create a new pin:\n  PIN_OBJECT_ID=FULL_GIT_OBJECT_ID\n  git -C {container}/main worktree add --detach {container}/pins/$PIN_OBJECT_ID $PIN_OBJECT_ID\n  # write {container}/pins/$PIN_OBJECT_ID.pin, then update the consumer\nAfter read-only checks prove every named consumer moved, retire the old pin:\n  pin-retire --consumer-main CONSUMER/main -- {container}/pins/{name}\nRaw removal stays denied; the old path and HEAD remain immutable until retirement.\n"
        )
    }
    fn base_env(&self) -> Env {
        if Path::new(&self.home).is_absolute() {
            BTreeMap::from([("HOME".into(), Some(self.home.clone()))])
        } else {
            Env::new()
        }
    }
    fn substitute(&self, word: &str, env: &Env) -> String {
        VARIABLE
            .replace_all(&self.expanduser(word), |m: &regex::Captures<'_>| {
                env.get(m.get(1).or_else(|| m.get(2)).unwrap().as_str())
                    .and_then(|s| s.as_ref())
                    .filter(|s| !s.is_empty())
                    .map_or_else(|| m[0].to_owned(), Clone::clone)
            })
            .into_owned()
    }
    fn target(&self, word: &str, env: &Env, cwd: &str) -> Option<(String, String)> {
        let value = self.substitute(word, env);
        let path = resolve(&value, cwd)?;
        let Some(cut) = value.find(['$', '`']) else {
            return Some((path.clone(), path));
        };
        if cut == 0 {
            return None;
        }
        let prefix = &value[..cut];
        let scope = resolve(prefix.rfind('/').map_or(".", |i| &prefix[..=i]), cwd)?;
        Some((path, scope))
    }
    fn deny(&self, path: &str, hit: &Hit, what: &str) -> String {
        format!(
            "This Bash command would {what} inside {} ({path}). {}\n\n{}\nReads are fine — it is the write that is refused.",
            hit.noun(),
            hit.why,
            self.advice(hit)
        )
    }
}
impl Hit {
    fn noun(&self) -> String {
        match &self.pin {
            Some(name) => format!("the externally-consumed pin {}/pins/{name}", self.project),
            None => format!("the PRIMARY checkout of {}", self.project),
        }
    }
}
fn redirect_targets(text: &str) -> Vec<String> {
    let mask: Vec<_> = unquoted_mask(text).chars().collect();
    let chars: Vec<_> = text.chars().collect();
    static TARGET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"^[\s\x1c-\x1f]*("[^"]+"|'[^']+'|[^\s\x1c-\x1f;&|<>]+)"#).unwrap()
    });
    let mut targets = Vec::new();
    let mut i = 0;
    while i < mask.len() {
        if mask[i] != '>' || (i > 0 && ['-', '&'].contains(&mask[i - 1])) {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        if mask.get(end) == Some(&'>') {
            end += 1;
        }
        if mask.get(end) == Some(&'&') {
            if end == i + 2 {
                end -= 1;
            } else {
                i += 1;
                continue;
            }
        }
        let tail: String = chars[end..].iter().collect();
        if let Some(m) = TARGET.captures(&tail) {
            targets.push(m[1].trim_matches(['"', '\'']).to_owned());
        }
        i = end;
    }
    for body in shell_substitutions(text) {
        targets.extend(redirect_targets(&body));
    }
    targets
}
fn record_assignments(words: &[String], env: &mut Env, policy: &Policy) {
    let index = words
        .iter()
        .position(|s| !CONTROL_WORDS.contains(&s.as_str()))
        .unwrap_or(words.len());
    let rest = &words[index..];
    if rest.len() > 1 && rest[0] == "for" {
        env.insert(rest[1].clone(), None);
        return;
    }
    if rest.first().is_some_and(|s| s == "read") {
        static NAME: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());
        for word in &rest[1..] {
            if NAME.is_match(word) {
                env.insert(word.clone(), None);
            }
        }
        return;
    }
    let assignments: Vec<_> = if rest.first().is_some_and(|s| {
        ["export", "local", "declare", "readonly", "typeset"].contains(&s.as_str())
    }) {
        rest[1..].iter().filter(|s| !s.starts_with('-')).collect()
    } else {
        rest.iter().collect()
    };
    if assignments.is_empty() || assignments.iter().any(|s| !ASSIGNMENT.is_match(s)) {
        return;
    }
    for word in assignments {
        let m = ASSIGNMENT.captures(word).unwrap();
        let value = policy.substitute(&m[2], env);
        env.insert(m[1].into(), (!value.contains(['$', '`'])).then_some(value));
    }
}
fn union(first: &[String], second: &[String]) -> Vec<String> {
    let mut all = first.to_vec();
    for value in second {
        if !all.contains(value) {
            all.push(value.clone());
        }
    }
    all
}
struct Segment {
    text: String,
    env: Env,
    cwds: Vec<String>,
}
impl Policy {
    fn cd_destinations(&self, words: &[String], env: &Env, cwds: &[String]) -> Vec<String> {
        let i = words
            .iter()
            .position(|s| !CONTROL_WORDS.contains(&s.as_str()))
            .unwrap_or(words.len());
        if i == words.len() || !["cd", "pushd"].contains(&words[i].as_str()) {
            return Vec::new();
        }
        let mut args = &words[i + 1..];
        while args.first().is_some_and(|s| s.starts_with('-') && s != "-") {
            let done = args[0] == "--";
            args = &args[1..];
            if done {
                break;
            }
        }
        let Some(arg) = args.first() else {
            return self
                .base_env()
                .get("HOME")
                .and_then(Clone::clone)
                .into_iter()
                .collect();
        };
        if arg == "-" || arg.starts_with('+') {
            return Vec::new();
        }
        let mut moved = Vec::new();
        for here in cwds {
            let Some((path, scope)) = self.target(arg, env, here) else {
                return Vec::new();
            };
            if path != scope {
                return Vec::new();
            }
            if !moved.contains(&path) {
                moved.push(path);
            }
        }
        moved
    }
    fn segments(&self, text: &str, initial: &Env, cwd: &str) -> Vec<Segment> {
        let mut env = initial.clone();
        let mut cwds = vec![cwd.to_owned()];
        let mut stack = Vec::new();
        let mut prev_sep: Option<String> = None;
        let mut out = Vec::new();
        for (text, sep) in shell::shell_segments_sep(text) {
            let tokens = tokens(&text);
            let mask = unquoted_mask(&text);
            let lead = text.len() - text.trim_start_matches(['(', ' ']).len();
            let opens = mask[..lead].matches('(').count();
            for _ in 0..opens {
                stack.push((true, cwds.clone()));
            }
            let words: Vec<_> = tokens
                .iter()
                .map(|s| s.trim_start_matches('(').to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            if words
                .first()
                .is_some_and(|s| BLOCK_OPEN.contains(&s.as_str()))
            {
                stack.push((false, cwds.clone()));
            }
            out.push(Segment {
                text,
                env: env.clone(),
                cwds: cwds.clone(),
            });
            record_assignments(&tokens, &mut env, self);
            if prev_sep.as_deref() != Some("|") && !matches!(sep.as_deref(), Some("|" | "&")) {
                let moved = self.cd_destinations(&words, &env, &cwds);
                if !moved.is_empty() {
                    cwds = if sep.as_deref() == Some("&&") {
                        moved
                    } else {
                        union(&cwds, &moved)
                    };
                }
            }
            let net = mask.matches('(').count() as isize
                - opens as isize
                - mask.matches(')').count() as isize;
            for _ in 0..net.max(0) {
                stack.push((true, cwds.clone()));
            }
            for _ in 0..(-net).max(0) {
                if let Some((subshell, saved)) = stack.pop() {
                    cwds = if subshell {
                        saved
                    } else {
                        union(&saved, &cwds)
                    };
                }
            }
            if words
                .first()
                .is_some_and(|s| BLOCK_CLOSE.contains(&s.as_str()))
                && let Some((_, saved)) = stack.pop() {
                    cwds = union(&saved, &cwds);
                }
            prev_sep = sep;
        }
        out
    }
}
fn leads_with(segment: &str, name: &str) -> bool {
    static ASSIGN_PREFIX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z_0-9]*=").unwrap());
    for word in tokens(segment) {
        if ASSIGN_PREFIX.is_match(&word) || ["env", "exec", "command"].contains(&word.as_str()) {
            continue;
        }
        return basename(&word) == name;
    }
    false
}
fn excise_sanctioned(text: &str) -> String {
    let mut out = String::new();
    let mut index = 0;
    for m in SEGMENT_SPLIT.find_iter(text) {
        let segment = &text[index..m.start()];
        if !leads_with(segment, "wt-rescue") {
            out.push_str(segment);
        }
        out.push_str(m.as_str());
        index = m.end();
    }
    if !leads_with(&text[index..], "wt-rescue") {
        out.push_str(&text[index..]);
    }
    out
}
pub(crate) fn patch_targets(envelope: &str) -> Vec<String> {
    PATCH_HEADER
        .captures_iter(envelope)
        .chain(PATCH_MOVE.captures_iter(envelope))
        .map(|m| m[1].into())
        .collect()
}
fn patch_removals(envelope: &str) -> Vec<String> {
    static BLOCK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\*\*\* (Add|Update|Delete) File:").unwrap());
    static DELETED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^\*\*\* (?:Delete|Update) File:[\s\x1c-\x1f]+(.+?)[\s\x1c-\x1f]*$")
            .unwrap()
    });
    let matches: Vec<_> = BLOCK.captures_iter(envelope).collect();
    let mut removed = Vec::new();
    for (i, m) in matches.iter().enumerate() {
        let start = m.get(0).unwrap().start();
        let end = matches
            .get(i + 1)
            .map_or(envelope.len(), |n| n.get(0).unwrap().start());
        let block = &envelope[start..end];
        if (&m[1] == "Delete" || (&m[1] == "Update" && PATCH_MOVE.is_match(block)))
            && let Some(header) = DELETED.captures(block)
        {
            removed.push(header[1].into());
        }
    }
    removed
}
fn find_envelope(raw: &serde_json::value::RawValue) -> Option<String> {
    if raw.get().starts_with('"') {
        let text: String = serde_json::from_str(raw.get()).ok()?;
        return text.contains(PATCH_BEGIN).then_some(text);
    }
    if !raw.get().starts_with('{') {
        return None;
    }
    struct Envelope;
    impl<'de> serde::de::Visitor<'de> for Envelope {
        type Value = Option<String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an object containing a patch envelope")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut entries: Vec<(String, Box<serde_json::value::RawValue>)> = Vec::new();
            while let Some((key, value)) =
                map.next_entry::<String, Box<serde_json::value::RawValue>>()?
            {
                if let Some((_, previous)) = entries.iter_mut().find(|(name, _)| name == &key) {
                    *previous = value;
                } else {
                    entries.push((key, value));
                }
            }
            Ok(entries
                .into_iter()
                .find_map(|(_, value)| find_envelope(&value)))
        }
    }
    serde::Deserializer::deserialize_map(
        &mut serde_json::Deserializer::from_str(raw.get()),
        Envelope,
    )
    .ok()
    .flatten()
}
fn invokes_apply_patch(text: &str) -> bool {
    SEGMENT_SPLIT
        .split(text)
        .any(|s| leads_with(s, "apply_patch"))
}
fn patch_fail_closed() -> String {
    "This apply_patch call is denied fail-closed: a patch whose targets cannot be read cannot be checked. Re-issue it with well-formed `*** Add/Update/Delete File:` headers. Deliberate bypass: `north config agents off launch-critical-worktree-guard`.".into()
}
impl Policy {
    fn patch_decision(&self, targets: &[String], cwd: &str, removals: &[String]) -> Option<String> {
        for target in removals {
            let path = resolve(&self.expanduser(target), cwd)?;
            if let Some(pin) = self.sidecar(&path) {
                return Some(format!(
                    "This apply_patch envelope would delete live pin consumer metadata at {path}. Retire the checkout and sidecar together after proving every named consumer moved:\n\n  pin-retire --consumer-main CONSUMER/main -- {pin}"
                ));
            }
        }
        for target in targets {
            if let Some(path) = resolve(&self.expanduser(target), cwd)
                && let Some(hit) = self.protected(&path)
            {
                return Some(format!(
                    "This apply_patch envelope would write {path} inside {}. {}\n\n{}",
                    hit.noun(),
                    hit.why,
                    self.advice(&hit)
                ));
            }
        }
        None
    }
    fn patch_tool(&self, input: &crate::pyjson::Value, cwd: &str, envelope: Option<&str>) -> Option<String> {
        let explicit: Vec<_> = ["file_path", "path"]
            .iter()
            .filter_map(|key| input[key].as_str().map(str::to_owned))
            .collect();
        if let Some(reason) = self.patch_decision(&explicit, cwd, &[]) {
            return Some(reason);
        }
        let selected = input["workdir"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| input["cwd"].as_str());
        let patch_cwd = selected.unwrap_or(cwd);
        let Some(envelope) = envelope else {
            return Some(patch_fail_closed());
        };
        let targets = patch_targets(envelope);
        if targets.is_empty() {
            return Some(patch_fail_closed());
        }
        self.patch_decision(&targets, patch_cwd, &patch_removals(envelope))
    }
    fn patch_command(&self, command: &crate::pyjson::Value, cwd: &str, stripped: Option<&str>) -> Option<String> {
        let (invoked, text) = if let Some(text) = command.as_str() {
            (
                invokes_apply_patch(stripped.unwrap_or(&strip_heredoc_bodies(text))),
                text.to_owned(),
            )
        } else if let Some(argv) = command.as_array() {
            let argv: Vec<_> = argv.iter().map(crate::pyjson::Value::as_str).collect::<Option<_>>()?;
            let mut invoked = argv.first().is_some_and(|s| basename(s) == "apply_patch");
            if !invoked
                && argv
                    .first()
                    .is_some_and(|s| ["bash", "sh", "zsh"].contains(&basename(s)))
            {
                for i in 1..argv.len().saturating_sub(1) {
                    let flag = argv[i];
                    if flag.starts_with('-') && flag[1..].contains('c') {
                        invoked = invokes_apply_patch(&strip_heredoc_bodies(argv[i + 1]));
                        break;
                    }
                }
            }
            (invoked, argv.join("\n"))
        } else {
            return None;
        };
        if !invoked {
            return None;
        }
        let envelopes: Vec<_> = PATCH_ENVELOPE.find_iter(&text).collect();
        if envelopes.is_empty() {
            return Some(patch_fail_closed());
        }
        for envelope in envelopes {
            let targets = patch_targets(envelope.as_str());
            if targets.is_empty() {
                return Some(patch_fail_closed());
            }
            if let Some(reason) =
                self.patch_decision(&targets, cwd, &patch_removals(envelope.as_str()))
            {
                return Some(reason);
            }
        }
        None
    }
}

fn git_output(args: &[&str]) -> Option<std::process::Output> {
    crate::platform::git::run_within(args, std::time::Duration::from_secs(5))
}
fn tracks_nothing(path: &str) -> bool {
    let parent = if Path::new(path).is_dir() {
        Path::new(path)
    } else {
        Path::new(path).parent().unwrap_or(Path::new(""))
    };
    let parent = parent.to_string_lossy();
    let Some(first) = git_output(&["-C", &parent, "ls-files", "--error-unmatch", "--", path])
    else {
        return false;
    };
    if first.status.success() {
        return false;
    }
    git_output(&["-C", &parent, "ls-files", "--", path]).is_some_and(|out| {
        out.status.success()
            && String::from_utf8_lossy(&out.stdout)
                .trim_matches(whitespace)
                .is_empty()
    })
}
fn shell_c_script(words: &[String], executable: usize) -> Option<&str> {
    let mut i = executable + 1;
    while i < words.len() {
        let arg = &words[i];
        if ["-O", "-o"].contains(&arg.as_str()) && i + 1 < words.len() {
            i += 2;
            continue;
        }
        if arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains('c') {
            i += 1;
            if words.get(i).is_some_and(|s| s == "--") {
                i += 1;
            }
            return words.get(i).map(String::as_str);
        }
        if arg == "--" || arg.starts_with('-') {
            i += 1;
            continue;
        }
        return None;
    }
    None
}
fn cargo_target_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for (segment, _) in shell::shell_segments_sep(text) {
        let words = tokens(&segment);
        let executable = shell::command_index(&words);
        if words
            .get(executable).is_none_or(|s| basename(s).trim_start_matches('(') != "cargo")
        {
            continue;
        }
        for word in &words[..executable] {
            if let Some(value) = word.strip_prefix("CARGO_TARGET_DIR=")
                && !value.is_empty()
                && !value.contains('$')
            {
                paths.push(value.into());
            }
        }
        let mut i = executable + 1;
        while i < words.len() {
            if words[i] == "--target-dir" && i + 1 < words.len() {
                let value = &words[i + 1];
                if !value.is_empty() && !value.contains('$') {
                    paths.push(value.clone());
                }
                i += 2;
                continue;
            }
            if let Some(value) = words[i].strip_prefix("--target-dir=")
                && !value.is_empty()
                && !value.contains('$')
            {
                paths.push(value.into());
            }
            i += 1;
        }
    }
    paths
}
struct GitCall {
    target: String,
    verb: String,
    args: Vec<String>,
}
impl Policy {
    fn git_invocations(&self, text: &str, cwd: &str, env: &Env) -> Vec<GitCall> {
        let mut found = Vec::new();
        for segment in self.segments(text, env, cwd) {
            let words = tokens(&segment.text);
            let executable = shell::command_index(&words);
            for here in &segment.cwds {
                for (i, word) in words.iter().enumerate() {
                    let name = basename(word).trim_start_matches('(');
                    if ["sh", "bash", "zsh"].contains(&name)
                        && i == executable
                        && let Some(script) = shell_c_script(&words, i).filter(|s| !s.is_empty())
                    {
                        found.extend(self.git_invocations(script, here, &segment.env));
                    }
                    if name != "git" {
                        continue;
                    }
                    let mut target = Some(here.clone());
                    let mut j = i + 1;
                    while j < words.len() {
                        let word = &words[j];
                        if word == "-C" && j + 1 < words.len() {
                            target = target.and_then(|target| {
                                self.target(&words[j + 1], &segment.env, &target)
                                    .map(|named| named.0)
                            });
                            j += 2;
                            continue;
                        }
                        if word.starts_with('-') {
                            j += 1;
                            continue;
                        }
                        if let Some(target) = target {
                            found.push(GitCall {
                                target,
                                verb: word.clone(),
                                args: words[j + 1..].to_vec(),
                            });
                        }
                        break;
                    }
                }
                for body in shell_substitutions(&segment.text) {
                    found.extend(self.git_invocations(&body, here, &segment.env));
                }
            }
        }
        found
    }
    fn worktree_decision(&self, call: &GitCall) -> Option<(String, String)> {
        let start = call.args.iter().position(|s| !s.starts_with('-'))?;
        let sub = call.args[start].as_str();
        let mut paths = Vec::new();
        let mut skip = false;
        let mut literal = false;
        for arg in &call.args[start + 1..] {
            if skip {
                skip = false;
                continue;
            }
            if literal {
                paths.push(arg);
                continue;
            }
            if arg == "--" {
                literal = true;
                continue;
            }
            if ["-b", "-B", "--reason"].contains(&arg.as_str()) {
                skip = true;
                continue;
            }
            if arg.starts_with('-') {
                continue;
            }
            paths.push(arg);
        }
        if sub == "add" {
            if self
                .protected(&call.target)
                .is_some_and(|hit| hit.pin.is_some())
            {
                return Some((
                    call.target.clone(),
                    "worktree add from an immutable pin".into(),
                ));
            }
            if let Some(raw) = paths.first() {
                let path = resolve(&self.expanduser(raw), &call.target)?;
                if let Some(hit) = self.protected(&path)
                    && !(hit.pin.is_some()
                        && Path::new(&path)
                            .parent()
                            .and_then(Path::file_name)
                            .is_some_and(|s| s == "pins"))
                {
                    return Some((path, "worktree add".into()));
                }
            }
        } else if sub == "remove" || sub == "move" {
            for raw in paths.iter().take(if sub == "remove" { 1 } else { 2 }) {
                let path = resolve(&self.expanduser(raw), &call.target)?;
                if let Some(hit) = self.protected(&path) {
                    let mut stray = false;
                    if sub == "remove"
                        && hit.pin.is_none()
                        && !call.args.iter().any(|s| s == "-f" || s == "--force")
                        && let Some(out) =
                            git_output(&["-C", &call.target, "worktree", "list", "--porcelain"])
                    {
                        let listed: Vec<_> = String::from_utf8_lossy(&out.stdout)
                            .lines()
                            .filter_map(|line| {
                                line.strip_prefix("worktree ")
                                    .and_then(|s| paths::realpath(Path::new(s)))
                            })
                            .collect();
                        stray = listed.first().is_some_and(|s| s != &path)
                            && listed.iter().skip(1).any(|s| s == &path);
                    }
                    if !stray {
                        return Some((path, format!("worktree {sub}")));
                    }
                }
            }
        }
        None
    }
    fn scan_write_commands(&self, words: &[String], cwd: &str, env: &Env) -> Option<String> {
        let package = words
            .first()
            .is_some_and(|s| PACKAGE_MANAGERS.contains(&basename(s)));
        let executable = shell::command_index(words);
        if words
            .get(executable)
            .is_some_and(|s| READ_ONLY_COMMANDS.contains(&basename(s)))
        {
            return None;
        }
        static REDIRECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d*(?:>>|>|<)").unwrap());
        for (i, word) in words.iter().enumerate() {
            let base = basename(word);
            if package && i > 0 {
                continue;
            }
            let args: Vec<_> = words[i + 1..]
                .iter()
                .filter(|s| !s.starts_with('-') && !REDIRECT.is_match(s))
                .collect();
            if base == "sed" && words[i + 1..].iter().any(|s| s.starts_with("-i")) {
                for arg in args.iter().skip(1) {
                    if let Some((path, _)) = self.target(arg, env, cwd)
                        && let Some(hit) = self.protected(&path)
                    {
                        return Some(self.deny(arg, &hit, "edit in place"));
                    }
                }
            } else if matches!(base, "perl" | "ruby")
                && words[i + 1..].iter().any(|s| interpreter_flags(s).0)
            {
                let scripts = words[i + 1..]
                    .iter()
                    .filter(|s| interpreter_flags(s).1)
                    .count()
                    .max(1);
                for arg in args.iter().skip(scripts) {
                    if let Some((path, _)) = self.target(arg, env, cwd)
                        && let Some(hit) = self.protected(&path)
                    {
                        return Some(self.deny(arg, &hit, "edit in place"));
                    }
                }
            } else if matches!(base, "python" | "python3") {
                for literal in python_write_targets(&words[i + 1..]) {
                    if let Some((path, _)) = self.target(&literal, env, cwd)
                        && let Some(hit) = self.protected(&path)
                    {
                        return Some(self.deny(&literal, &hit, "write a file from an inline script"));
                    }
                }
            } else if WRITE_COMMANDS.contains(&base) || DESTRUCTIVE_COMMANDS.contains(&base) {
                for arg in args {
                    let Some((path, scope)) = self.target(arg, env, cwd) else {
                        continue;
                    };
                    if DESTRUCTIVE_COMMANDS.contains(&base)
                        && let Some(pin) = self.sidecar(&path)
                        && let Some(hit) = self.protected(&pin)
                    {
                        return Some(self.deny(&path, &hit, &format!("delete live consumer metadata with `{base}`; use `pin-retire --consumer-main CONSUMER/main -- {pin}` instead")));
                    }
                    if let Some(hit) = self.protected(&path) {
                        if DESTRUCTIVE_COMMANDS.contains(&base)
                            && hit.pin.is_none()
                            && tracks_nothing(&scope)
                        {
                            continue;
                        }
                        return Some(self.deny(arg, &hit, &format!("run `{base}`")));
                    }
                }
            } else if COPY_COMMANDS.contains(&base) && !args.is_empty() {
                if base == "mv" {
                    for source in &args[..args.len() - 1] {
                        if let Some((path, _)) = self.target(source, env, cwd)
                            && let Some(pin) = self.sidecar(&path)
                            && let Some(hit) = self.protected(&pin)
                        {
                            return Some(self.deny(&path, &hit, &format!("move live consumer metadata; use `pin-retire --consumer-main CONSUMER/main -- {pin}` instead")));
                        }
                    }
                }
                let dest = args.last().unwrap();
                if REMOTE_COPY_COMMANDS.contains(&base) && remote_spec(dest) {
                    continue;
                }
                if let Some((path, _)) = self.target(dest, env, cwd)
                    && let Some(hit) = self.protected(&path)
                {
                    return Some(self.deny(dest, &hit, &format!("run `{base}` into")));
                }
            }
        }
        None
    }
    fn decide(&self, input: &HookInput) -> Option<String> {
        let tool = input.data["tool_name"].as_str().unwrap_or("");
        let ti = &input.data["tool_input"];
        let current = std::env::current_dir().ok()?.to_string_lossy().into_owned();
        let cwd = input.data["cwd"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&current);
        if tool.ends_with("apply_patch") {
            let fields: BTreeMap<String, Box<serde_json::value::RawValue>> =
                serde_json::from_slice(&input.raw).ok()?;
            let envelope = fields.get("tool_input").and_then(|raw| find_envelope(raw));
            return self.patch_tool(ti, cwd, envelope.as_deref());
        }
        if !input.is_shell() {
            let path = ti["file_path"].as_str()?;
            let hit = self.protected(path)?;
            return Some(format!(
                "{path} is inside {}. {}\n\n{}",
                hit.noun(),
                hit.why,
                self.advice(&hit)
            ));
        }
        let command = &ti["command"];
        if command.as_array().is_some()
            && let Some(reason) = self.patch_command(command, cwd, None)
        {
            return Some(reason);
        }
        let command_text = input.command()?.as_str().filter(|s| !s.is_empty())?;
        static CD: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?:^|[;&|]|&&)[\s\x1c-\x1f]*cd[\s\x1c-\x1f]+("[^"]+"|'[^']+'|[^\s\x1c-\x1f;&|]+)"#).unwrap()
        });
        let effective = CD
            .captures(command_text)
            .and_then(|m| resolve(&self.expanduser(m[1].trim_matches(['"', '\''])), cwd))
            .unwrap_or_else(|| cwd.into());
        let scan = excise_sanctioned(&strip_heredoc_bodies(command_text));
        if let Some(reason) = self.patch_command(command, &effective, Some(&scan)) {
            return Some(reason);
        }
        let words = tokens(&scan);
        let calls = self.git_invocations(&scan, cwd, &self.base_env());
        let segments = self.segments(&scan, &self.base_env(), cwd);
        let mut final_env = segments
            .last()
            .map_or_else(|| self.base_env(), |s| s.env.clone());
        if let Some(last) = segments.last() {
            record_assignments(&tokens(&last.text), &mut final_env, self);
        }
        let mut visited = Vec::new();
        for segment in &segments {
            visited = union(&visited, &segment.cwds);
        }
        if visited.is_empty() {
            visited.push(cwd.into());
        }
        let mut nested = Vec::new();
        for body in shell_substitutions(&scan) {
            for here in &visited {
                nested.extend(self.segments(&body, &final_env, here));
            }
        }
        for raw in cargo_target_paths(&scan) {
            let path = resolve(&self.expanduser(&raw), &effective)?;
            if let Some(hit) = self.protected(&path) {
                return Some(self.deny(&path, &hit, "write Cargo target output"));
            }
            if let Some(project) = self.spill(&path) {
                let container = format!("{}/{project}", self.root);
                return Some(format!(
                    "This Bash command would put Cargo target output directly in the repository container {container} ({path}). Build output belongs to the exact lane that produced it. Use the lane-local default, set `CARGO_TARGET_DIR={container}/worktrees/SLUG/target`, or use a deliberate /tmp target outside repository containers."
                ));
            }
        }
        for call in &calls {
            if let Some(what) = wip_destroying(&call.verb, &call.args)
                && let Some(hit) = self.protected(&call.target)
            {
                let target = &call.target;
                if hit.pin.is_some() {
                    return Some(format!(
                        "`{what}` targets {target}, {}. {}\n\n{}",
                        hit.noun(),
                        hit.why,
                        self.advice(&hit)
                    ));
                }
                return Some(format!(
                    "`{what}` targets {target}, the MAIN checkout of {}. Uncommitted state in a main checkout is the human's work-in-progress; an agent never discards it. {}\n\nCompliant moves:\n  git -C {target} status --porcelain   # inspect, do not discard\n  wt-rescue {target}\n  # dirty main? run `wt-rescue` (relocates intact, restores clean)\n  # rare surgery only: `north config agents off launch-critical-worktree-guard` — deliberate\n  # bypass, state why, re-enable after\n{}",
                    hit.project,
                    hit.why,
                    self.worktree_advice(&hit.project)
                ));
            }
        }
        for call in calls {
            let decision = if call.verb == "worktree" {
                self.worktree_decision(&call)
            } else if call.verb == "fetch" || git_read_form(&call.verb, &call.args) {
                None
            } else if ["merge", "pull"].contains(&call.verb.as_str()) {
                (!call.args.iter().any(|s| s == "--ff-only")).then(|| {
                    (
                        call.target.clone(),
                        format!("{} (without --ff-only)", call.verb),
                    )
                })
            } else {
                MUTATING_GIT
                    .contains(&call.verb.as_str())
                    .then(|| (call.target.clone(), call.verb.clone()))
            };
            if let Some((path, verb)) = decision
                && let Some(hit) = self.protected(&path)
            {
                return Some(self.deny(&path, &hit, &format!("run `git {verb}`")));
            }
        }
        for segment in &segments {
            for raw in redirect_targets(&segment.text) {
                for here in &segment.cwds {
                    if let Some((path, _)) = self.target(&raw, &segment.env, here)
                        && let Some(hit) = self.protected(&path)
                    {
                        return Some(self.deny(&path, &hit, "write"));
                    }
                }
            }
        }
        for segment in segments.iter().chain(&nested) {
            for here in &segment.cwds {
                if let Some(reason) =
                    self.scan_write_commands(&tokens(&segment.text), here, &segment.env)
                {
                    return Some(reason);
                }
            }
        }
        if let Some(hit) = self.protected(&effective)
            && scan.contains("<<")
            && words.iter().any(|s| INTERPRETERS.contains(&basename(s)))
        {
            return Some(self.deny(
                &effective,
                &hit,
                "run an interpreter script with its working directory",
            ));
        }
        None
    }
}
fn interpreter_flags(word: &str) -> (bool, bool) {
    let Some(letters) = word.strip_prefix('-').filter(|s| !s.starts_with('-')) else {
        return (false, false);
    };
    for letter in letters.chars() {
        match letter {
            'i' => return (true, false),
            'e' | 'E' => return (false, true),
            'M' | 'm' | 'I' | 'x' | 'C' | 'F' | 'd' | 'D' | 'r' | 'K' | 'T' => break,
            _ => {}
        }
    }
    (false, false)
}
fn python_write_targets(words: &[String]) -> Vec<String> {
    static WRITES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\.(?:write|writelines|write_text|write_bytes)\s*\(|\bopen\s*\([^()]*,\s*(?:mode\s*=\s*)?['"][rbt]*[wax+]"#).unwrap()
    });
    static LITERAL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"'([^']*)'|"([^"]*)""#).unwrap());
    let Some(at) = words.iter().position(|s| s == "-c") else {
        return Vec::new();
    };
    let Some(code) = words.get(at + 1).filter(|code| WRITES.is_match(code)) else {
        return Vec::new();
    };
    LITERAL
        .captures_iter(code)
        .filter_map(|capture| capture.get(1).or_else(|| capture.get(2)))
        .map(|literal| literal.as_str().to_owned())
        .filter(|literal| literal.contains(['/', '.']))
        .chain(
            words[at + 2..]
                .iter()
                .filter(|s| !s.starts_with('-'))
                .cloned(),
        )
        .collect()
}
fn short_letters(args: &[String]) -> Vec<char> {
    args.iter()
        .filter(|s| {
            s.starts_with('-') && s.len() > 1 && s[1..].chars().all(|c| c.is_ascii_alphabetic())
        })
        .flat_map(|s| s[1..].chars())
        .collect()
}
fn git_read_form(verb: &str, args: &[String]) -> bool {
    match verb {
        "stash" => args
            .iter()
            .find(|s| !s.starts_with('-'))
            .is_some_and(|s| ["list", "show"].contains(&s.as_str())),
        "clean" => args.iter().any(|s| s == "--dry-run") || short_letters(args).contains(&'n'),
        _ => false,
    }
}
fn wip_destroying(verb: &str, args: &[String]) -> Option<String> {
    let has = |flag: &str| args.iter().any(|s| s == flag);
    match verb {
        "reset" => ["--hard", "--merge", "--keep"]
            .into_iter()
            .find(|s| has(s))
            .map(|s| format!("git reset {s}")),
        "stash" if !git_read_form(verb, args) => Some(format!(
            "git stash {}",
            args.iter()
                .find(|s| !s.starts_with('-'))
                .map_or("push", String::as_str)
        )),
        "checkout" if has("--") || has(".") || has("-f") || has("--force") => {
            Some("git checkout of working-tree paths".into())
        }
        "restore" if !has("--staged") || has("--worktree") => Some("git restore".into()),
        "clean"
            if !git_read_form(verb, args)
                && (has("--force") || short_letters(args).contains(&'f')) =>
        {
            Some("git clean -f".into())
        }
        _ => None,
    }
}
pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::hook::authoring_guards_off(ID) {
        return Verdict::Allow;
    }
    let raw = String::from_utf8_lossy(&input.raw);
    if std::env::var("LAUNCH_CRITICAL_CODE_ROOT")
        .unwrap_or_default()
        .is_empty()
        && !raw.contains("apply_patch")
        && !raw.contains("Begin Patch")
        && (!raw.contains("/code/")
            || ![
                "main",
                "pins",
                "/code/north",
                "/code/nixos-config",
                "CARGO_TARGET_DIR",
                "--target-dir",
            ]
            .iter()
            .any(|s| raw.contains(s)))
        {
            return Verdict::Allow;
        }
    let policy = Policy::from_env();
    let Some(reason) = policy.decide(input) else {
        return Verdict::Allow;
    };
    if !raw.contains("/pins/")
        && let Some(path) = input.data["tool_input"]["file_path"]
            .as_str()
            .filter(|s| !s.is_empty())
    {
        let parent = Path::new(path).parent().unwrap_or(Path::new(""));
        if git_output(&["-C", &parent.to_string_lossy(), "check-ignore", "-q", path])
            .is_some_and(|out| out.status.success())
        {
            return Verdict::Allow;
        }
    }
    Verdict::Deny(reason)
}

pub fn record_invalid_input(bytes: &[u8]) {
    let Ok(data) = serde_json::from_slice::<Value>(bytes) else {
        return;
    };
    if data["hook_event_name"]
        .as_str()
        .is_some_and(|event| event != "PreToolUse")
    {
        return;
    }
    let input = data.get("tool_input").unwrap_or(&Value::Null);
    if input.is_null() || input.is_object() || crate::hook::authoring_guards_off(ID) {
        return;
    }
    let raw = String::from_utf8_lossy(bytes);
    if std::env::var("LAUNCH_CRITICAL_CODE_ROOT")
        .unwrap_or_default()
        .is_empty()
        && !raw.contains("apply_patch")
        && !raw.contains("Begin Patch")
        && (!raw.contains("/code/")
            || ![
                "main",
                "pins",
                "/code/north",
                "/code/nixos-config",
                "CARGO_TARGET_DIR",
                "--target-dir",
            ]
            .iter()
            .any(|s| raw.contains(s)))
    {
        return;
    }
    let path = std::env::var("AGENT_HOOK_ERRORS").unwrap_or_else(|_| {
        format!(
            "{}/.local/state/agents/hooks/errors.tsv",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    let path = Path::new(&path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{}\tlaunch-critical-worktree-guard\tdecider-exception", Timestamp::now().iso());
    }
}

#[cfg(test)]
#[path = "launch_critical_tests.rs"]
mod tests;
