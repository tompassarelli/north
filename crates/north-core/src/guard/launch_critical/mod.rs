mod decide;
mod git;
mod patch;
mod shell;
mod writes;

use git::git_output;
pub use patch::patch_targets;
use shell::resolve;
use crate::foundation::clock::Timestamp;
use crate::guard::paths;
use crate::hook::{HookInput, Verdict};
use crate::shell::whitespace;
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
            "A pin is not a lane. Do not edit it, and do not cut a worktree from it:\n  cat {container}/pins/{name}.pin        # who consumes it, on what terms\nTo advance a consumer, leave this checkout untouched and create a new pin:\n  PIN_OBJECT_ID=FULL_GIT_OBJECT_ID\n  git -C {container}/main worktree add --detach {container}/pins/$PIN_OBJECT_ID $PIN_OBJECT_ID\n  # write {container}/pins/$PIN_OBJECT_ID.pin, then update the consumer\nAfter read-only checks prove every named consumer moved, retire the old pin:\n  north pin-retire --consumer-main CONSUMER/main -- {container}/pins/{name}\nRaw removal stays denied; the old path and HEAD remain immutable until retirement.\n"
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
pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off(ID) {
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
    if input.is_null() || input.is_object() || crate::guard::activation::authoring_guards_off(ID) {
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
#[path = "../launch_critical_tests.rs"]
mod tests;
