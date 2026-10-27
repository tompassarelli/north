use crate::guard::paths::{basename, dirname, expand_home, join, normpath, realpath};
use crate::hook::{HookInput, Verdict};
use crate::pyjson::Value;
use crate::shell::{command_index, is_separator, words};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

const ID: &str = "north-hook";
const POLICY: &str = "Never hand-edit generated agent policy (~/.claude/CLAUDE.md, ~/.codex). Edit south:agents/ or north:agent-machinery/ and run `agents sync`.";
const NIX: &str = "Never hand-edit a .nix generated from .bnix. Edit the .bnix and regenerate the .nix from it.";
const UNKNOWN_DIR: &str = "Can't resolve the directory after `cd`/`popd`, so this write to a CLAUDE.md, AGENTS.md or SKILL.md may hit generated agent policy. Use an absolute path; for ~/.claude or ~/.codex edit south:agents/ or north:agent-machinery/ and run `agents sync`.";
const MENTIONS: [&str; 6] = ["CLAUDE.md", "AGENTS.md", "SKILL.md", ".claude", ".codex", ".nix"];
const PROTECTED_NAMES: [&str; 3] = ["CLAUDE.md", "AGENTS.md", "SKILL.md"];
const EVERY_OPERAND: [&str; 8] = ["sed", "sd", "tee", "truncate", "perl", "ruby", "ed", "ex"];
const LAST_OPERAND: [&str; 5] = ["cp", "mv", "install", "ln", "rsync"];
const PATCH_BEGIN: &str = "*** Begin Patch";

static REDIRECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d*(?:>>|>\||>)(.*)\n?\z").unwrap());
static SED_IN_PLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:--in-place|-[a-zA-Z]*i)").unwrap());
static PERL_IN_PLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-[0-9nplaswtTUWXcS]*i").unwrap());
static RUBY_IN_PLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-[0-9nplasw]*i").unwrap());

fn unresolved(c: char) -> bool {
    matches!(c, '$' | '`' | '*' | '?' | '[' | '{')
}

fn redirect_operand(token: &str) -> Option<&str> {
    REDIRECT
        .captures(token)
        .map(|captures| captures.get(1).map_or("", |m| m.as_str()))
}

fn glob_class(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut end = start;
    if chars.get(end) == Some(&'!') {
        end += 1;
    }
    if chars.get(end) == Some(&']') {
        end += 1;
    }
    while end < chars.len() && chars[end] != ']' {
        end += 1;
    }
    if end >= chars.len() {
        return None;
    }
    let mut set = &chars[start..end];
    let mut out = String::from("[");
    if set.first() == Some(&'!') {
        out.push('^');
        set = &set[1..];
    }
    let mut k = 0;
    while k < set.len() {
        out.push_str(&regex::escape(&set[k].to_string()));
        if k + 2 < set.len() && set[k + 1] == '-' {
            out.push('-');
            out.push_str(&regex::escape(&set[k + 2].to_string()));
            k += 3;
        } else {
            k += 1;
        }
    }
    out.push(']');
    Some((out, end + 1))
}

fn glob_regex(pattern: &str) -> Option<Regex> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::from("(?s)^");
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '[' => match glob_class(&chars, i) {
                Some((class, next)) => {
                    out.push_str(&class);
                    i = next;
                }
                None => out.push_str("\\["),
            },
            _ => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    out.push_str("\\z");
    Regex::new(&out).ok()
}

struct Target {
    raw: String,
    base: Option<String>,
    fuzzy: bool,
}

impl Target {
    fn plain(raw: &str) -> Self {
        Self { raw: raw.to_owned(), base: None, fuzzy: false }
    }

    fn at(raw: &str, state: &State) -> Self {
        Self { raw: raw.to_owned(), base: Some(state.base.clone()), fuzzy: state.fuzzy }
    }
}

#[derive(Clone)]
struct State {
    base: String,
    fuzzy: bool,
    previous: Option<(String, bool)>,
    stack: Vec<(String, bool)>,
}

pub struct Policy {
    home: String,
    claude: String,
    codex: String,
    cwd: String,
}

impl Policy {
    pub fn new(home: &str, cwd: &str) -> Self {
        Self {
            home: home.to_owned(),
            claude: join(home, ".claude"),
            codex: join(home, ".codex"),
            cwd: cwd.to_owned(),
        }
    }

    fn located(&self, raw: &str, base: Option<&str>) -> String {
        let path = expand_home(raw, &self.home);
        if path.starts_with('/') {
            return normpath(&path);
        }
        let base = base.filter(|b| !b.is_empty()).unwrap_or(&self.cwd);
        normpath(&join(base, &path))
    }

    fn protected_glob(&self, path: &str) -> bool {
        let name = basename(path);
        if !name.chars().any(unresolved) {
            return false;
        }
        let parent = dirname(path);
        if parent != self.claude && parent != self.codex {
            return false;
        }
        name.chars().any(|c| matches!(c, '$' | '`' | '{'))
            || glob_regex(name)
                .is_some_and(|glob| PROTECTED_NAMES.iter().any(|n| glob.is_match(n)))
    }

    fn reason(&self, target: &Target) -> Option<&'static str> {
        let raw = target.raw.as_str();
        if raw.is_empty() {
            return None;
        }
        let expanded = expand_home(raw, &self.home);
        if target.fuzzy && !expanded.starts_with('/') {
            if PROTECTED_NAMES.contains(&basename(raw)) {
                return Some(UNKNOWN_DIR);
            }
            let base = target.base.as_deref().unwrap_or("");
            if ["skills", "prompts"]
                .iter()
                .any(|d| base.starts_with(&join(&self.codex, d)))
            {
                return Some(POLICY);
            }
        }
        let path = self.located(raw, target.base.as_deref());
        if path == join(&self.claude, "CLAUDE.md") || path == join(&self.codex, "AGENTS.md") {
            return Some(POLICY);
        }
        if self.protected_glob(&path) {
            return Some(POLICY);
        }
        for generated in ["skills", "prompts"] {
            let root = join(&self.codex, generated);
            if path == root || path.starts_with(&format!("{root}/")) {
                return Some(POLICY);
            }
        }
        let resolved = realpath(Path::new(&path));
        for candidate in std::iter::once(Some(path)).chain(std::iter::once(resolved)).flatten() {
            if let Some(stem) = candidate.strip_suffix(".nix")
                && Path::new(&format!("{stem}.bnix")).exists()
            {
                return Some(NIX);
            }
        }
        None
    }

    fn changed_dir(&self, raw: &str, base: &str, fuzzy: bool) -> (String, bool) {
        let expanded = expand_home(raw, &self.home);
        if let Some(at) = expanded.find(unresolved) {
            let literal = &expanded[..at];
            let directory = if literal.is_empty() {
                "."
            } else if literal.ends_with('/') {
                literal
            } else {
                dirname(literal)
            };
            return (self.located(directory, Some(base)), true);
        }
        let fuzzy = fuzzy && !expanded.starts_with('/');
        (self.located(&expanded, Some(base)), fuzzy)
    }

    fn shell_targets(&self, command: &str) -> Vec<Target> {
        let mut out = Vec::new();
        let Some(mut tokens) = words(command) else {
            return out;
        };
        tokens.push(";".into());
        let mut state = State {
            base: self.cwd.clone(),
            fuzzy: false,
            previous: None,
            stack: Vec::new(),
        };
        let mut saved: Vec<State> = Vec::new();
        let mut segment: Vec<String> = Vec::new();
        for token in tokens {
            if !is_separator(&token) {
                segment.push(token);
                continue;
            }
            if !segment.is_empty() {
                state = self.segment_targets(&segment, state, &mut out);
            }
            segment.clear();
            for c in token.chars() {
                if c == '(' {
                    saved.push(state.clone());
                } else if c == ')'
                    && let Some(top) = saved.pop()
                {
                    state = top;
                }
            }
        }
        out
    }

    fn segment_targets(&self, segment: &[String], state: State, out: &mut Vec<Target>) -> State {
        redirect_targets(segment, &state, out);
        let start = command_index(segment);
        if start >= segment.len() {
            return state;
        }
        let name = basename(&segment[start]);
        let args = &segment[start + 1..];
        if matches!(name, "cd" | "pushd" | "popd") {
            return self.change_directory(name, args, state);
        }
        if !writes_in_place(name, args) {
            return state;
        }
        operand_targets(name, args, &state, out);
        state
    }

    fn change_directory(&self, name: &str, args: &[String], state: State) -> State {
        let operands: Vec<&String> = args
            .iter()
            .filter(|w| !w.starts_with('-') || w.as_str() == "-")
            .collect();
        let here = (state.base.clone(), state.fuzzy);
        let mut stack = state.stack.clone();
        let target = if name == "popd" {
            stack.pop().unwrap_or((state.base.clone(), true))
        } else if operands.is_empty() {
            (self.home.clone(), false)
        } else if operands[0] == "-" {
            state.previous.clone().unwrap_or((state.base.clone(), true))
        } else {
            self.changed_dir(operands[0], &state.base, state.fuzzy)
        };
        if name == "pushd" {
            stack.push(here.clone());
        }
        State { base: target.0, fuzzy: target.1, previous: Some(here), stack }
    }

    fn file_targets(&self, input: &HookInput) -> Vec<Target> {
        let tool = input.data["tool_name"].as_str().unwrap_or("");
        if input.is_shell() {
            return input
                .command()
                .map_or_else(Vec::new, |command| self.shell_targets(&command.lossy()));
        }
        if tool.ends_with("apply_patch") {
            return find_envelope(&input.tool_input)
                .map_or_else(Vec::new, |envelope| {
                    crate::guard::launch_critical::patch_targets(envelope)
                        .iter()
                        .map(|raw| Target::plain(raw))
                        .collect()
                });
        }
        if matches!(input.tool_name(), "Edit" | "Write" | "MultiEdit" | "NotebookEdit") {
            return ["file_path", "filePath", "notebook_path"]
                .iter()
                .filter_map(|key| input.tool_input[key].as_str())
                .map(Target::plain)
                .collect();
        }
        Vec::new()
    }

    pub fn decide(&self, input: &HookInput) -> Option<&'static str> {
        self.file_targets(input).iter().find_map(|target| self.reason(target))
    }
}

fn redirect_targets(segment: &[String], state: &State, out: &mut Vec<Target>) {
    for (index, token) in segment.iter().enumerate() {
        if let Some(operand) = redirect_operand(token) {
            let raw = if operand.is_empty() {
                segment.get(index + 1).map_or("", String::as_str)
            } else {
                operand
            };
            out.push(Target::at(raw, state));
        }
    }
}

fn writes_in_place(name: &str, args: &[String]) -> bool {
    let pattern = match name {
        "sed" => &*SED_IN_PLACE,
        "perl" => &*PERL_IN_PLACE,
        "ruby" => &*RUBY_IN_PLACE,
        "awk" | "gawk" => return args.iter().any(|w| w == "inplace" || w == "-iinplace"),
        _ => return true,
    };
    args.iter().any(|w| w.starts_with('-') && pattern.is_match(w))
}

fn split_operands<'a>(name: &str, args: &'a [String]) -> (Option<&'a str>, Vec<&'a str>) {
    let mut directory: Option<&str> = None;
    let mut operands: Vec<&str> = Vec::new();
    let mut skip = false;
    for (position, word) in args.iter().enumerate() {
        if skip {
            skip = false;
        } else if matches!(word.as_str(), "-t" | "--target-directory")
            && LAST_OPERAND.contains(&name)
            && position + 1 < args.len()
        {
            directory = Some(&args[position + 1]);
            skip = true;
        } else if let Some(value) = word.strip_prefix("--target-directory=") {
            directory = Some(value);
        } else if !word.starts_with('-') && redirect_operand(word).is_none() {
            operands.push(word);
        }
    }
    (directory, operands)
}

fn operand_targets(name: &str, args: &[String], state: &State, out: &mut Vec<Target>) {
    let (directory, mut operands) = split_operands(name, args);
    if EVERY_OPERAND.contains(&name) || matches!(name, "awk" | "gawk") {
        out.extend(operands.iter().map(|w| Target::at(w, state)));
    } else if LAST_OPERAND.contains(&name) && (!operands.is_empty() || directory.is_some()) {
        let directory = match directory {
            Some(directory) => directory,
            None => operands.pop().unwrap_or(""),
        };
        out.push(Target::at(directory, state));
        for operand in operands {
            let leaf = basename(operand.trim_end_matches('/'));
            out.push(Target::at(&join(directory, leaf), state));
        }
    } else if name == "dd" {
        out.extend(
            operands
                .iter()
                .filter_map(|w| w.strip_prefix("of="))
                .map(|w| Target::at(w, state)),
        );
    }
}

fn find_envelope(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => text.as_str().filter(|s| s.contains(PATCH_BEGIN)),
        Value::Object(entries) => entries.iter().enumerate().find_map(|(index, (key, _))| {
            if entries[..index].iter().any(|(earlier, _)| earlier == key) {
                return None;
            }
            let last = entries.iter().rfind(|(later, _)| later == key)?;
            find_envelope(&last.1)
        }),
        _ => None,
    }
}

pub fn evaluate(input: &HookInput) -> Verdict {
    let raw = String::from_utf8_lossy(&input.raw);
    if !MENTIONS.iter().any(|mention| raw.contains(mention)) {
        return Verdict::Allow;
    }
    if crate::hook::authoring_guards_off(ID) {
        return Verdict::Allow;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let cwd = input
        .cwd()
        .map(str::to_owned)
        .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_default();
    match Policy::new(&home, &cwd).decide(input) {
        Some(reason) => Verdict::Deny(reason.into()),
        None => Verdict::Allow,
    }
}

#[cfg(test)]
#[path = "generated_policy_tests.rs"]
mod tests;
