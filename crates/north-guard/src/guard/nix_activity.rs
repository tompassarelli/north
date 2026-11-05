use super::file_skill::claim;
use super::handoff_record::edited_paths;
use crate::hook::{HookInput, Verdict};
use crate::shell;
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

const TABLE: &str = include_str!("../../../../harness/nix/cards.toml");
const PREFIXES: &[&str] = &["exec", "command", "nohup", "setsid", "nice", "ionice", "stdbuf", "timeout", "time"];
const LAUNCH_MARKERS: &[&str] = &["-c", "--command", "--run", "--", "exec", "env", "run"];
const VALUE_FLAGS: &[&str] = &["--extra-experimental-features", "--option", "--extra-substituters", "--store", "--eval-store", "-I", "--include", "--max-jobs", "-j", "--cores"];
const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash"];
const DEPTH: usize = 4;

pub struct Deny {
    wrappers: Vec<String>,
    inner: Vec<String>,
    commands: Vec<String>,
    dev_shell: bool,
    why: String,
}

pub struct Card {
    pub class: String,
    pub incident: String,
    pub title: String,
    pub text: String,
    pub explain: Vec<String>,
    pub options: Vec<String>,
    commands: Vec<String>,
    path_dirs: Vec<String>,
    path_suffixes: Vec<String>,
    deny: Vec<Deny>,
}

impl Card {
    pub fn render(&self) -> String {
        format!("nix-activity: {} {} ({})\n{}\nmore: `north nix explain {}`", self.class, self.title, self.incident, self.text.trim_end(), self.class)
    }
}

fn strings(table: &toml_edit::Table, key: &str) -> Vec<String> {
    table.get(key).and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str()).map(str::to_owned).collect()
}

fn text(table: &toml_edit::Table, key: &str) -> String {
    table.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_owned()
}

pub fn cards() -> Vec<Card> {
    let Ok(doc) = TABLE.parse::<DocumentMut>() else {
        return Vec::new();
    };
    doc["card"]
        .as_array_of_tables()
        .into_iter()
        .flatten()
        .map(|table| Card {
            class: text(table, "class"),
            incident: text(table, "incident"),
            title: text(table, "title"),
            text: text(table, "text"),
            explain: strings(table, "explain"),
            options: strings(table, "options"),
            commands: strings(table, "commands"),
            path_dirs: strings(table, "path_dirs"),
            path_suffixes: strings(table, "path_suffixes"),
            deny: table
                .get("deny")
                .and_then(|v| v.as_array_of_tables())
                .into_iter()
                .flatten()
                .map(|deny| Deny { wrappers: strings(deny, "wrappers"), inner: strings(deny, "inner"), commands: strings(deny, "commands"), dev_shell: deny.get("dev_shell").and_then(|v| v.as_bool()).unwrap_or(false), why: text(deny, "why") })
                .collect(),
        })
        .collect()
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    decide(input, &seen_root())
}

fn seen_root() -> PathBuf {
    let get = |name: &str| std::env::var(name).ok().filter(|s| !s.is_empty());
    let state = get("XDG_STATE_HOME").unwrap_or_else(|| format!("{}/.local/state", get("HOME").unwrap_or_default()));
    Path::new(&state).join("north/nix-cards")
}

pub(super) fn decide(input: &HookInput, seen: &Path) -> Verdict {
    let cards = cards();
    let hits: Vec<&Card> = if input.is_shell() {
        let Some(command) = input.command().map(|c| c.lossy()) else {
            return Verdict::Allow;
        };
        let runs = runs(&command, 0);
        for card in &cards {
            for deny in card.deny.iter().filter(|deny| refuses(deny, &runs)) {
                if !deny.dev_shell {
                    return Verdict::Deny(format!("nix-activity: refused, {}.\n{}", deny.why, card.render()));
                }
                if let Some(flake) = input.data["cwd"].as_str().and_then(|cwd| dev_shell_flake(Path::new(cwd))) {
                    return Verdict::Deny(format!("nix-activity: refused, {}: {} has a dev shell (devShells), so add the package to it and run through `north dev-shell run -- COMMAND`.\n{}", deny.why, flake.display(), card.render()));
                }
            }
        }
        cards.iter().filter(|card| card.commands.iter().any(|spec| runs.iter().any(|words| launches(words, spec)))).collect()
    } else if matches!(input.tool_name(), "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch") {
        let cwd = input.data["cwd"].as_str().map(PathBuf::from).unwrap_or_default();
        let paths: Vec<String> = edited_paths(input).into_iter().map(|p| cwd.join(p).display().to_string()).collect();
        cards.iter().filter(|card| paths.iter().any(|path| edits(card, path, input))).collect()
    } else {
        return Verdict::Allow;
    };
    let Some(session) = input.data["session_id"].as_str().filter(|s| !s.is_empty()) else {
        return Verdict::Allow;
    };
    let notes: Vec<String> = hits.into_iter().filter(|card| claim(seen, session, &card.class)).map(Card::render).collect();
    match notes.is_empty() {
        true => Verdict::Allow,
        false => Verdict::Advise(notes.join("\n\n")),
    }
}

fn edits(card: &Card, path: &str, input: &HookInput) -> bool {
    card.path_suffixes.iter().any(|suffix| path.ends_with(suffix.as_str()))
        || (card.path_dirs.iter().any(|dir| path.contains(&format!("/{dir}"))) && {
            let written = ["content", "new_string", "command", "input", "patch"].iter().filter_map(|k| input.tool_input[*k].as_str()).collect::<Vec<_>>().join("\n");
            let disk = std::fs::read_to_string(path).unwrap_or_default();
            card.commands.iter().any(|spec| written.contains(spec.as_str()) || disk.contains(spec.as_str()))
        })
}

fn runs(source: &str, depth: usize) -> Vec<Vec<String>> {
    let text = shell::strip_heredocs(source);
    let Some(tokens) = shell::words(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if depth < DEPTH {
        for body in shell::shell_substitutions(&text) {
            out.extend(runs(&body, depth + 1));
        }
    }
    for segment in shell::segments(&tokens) {
        let words: Vec<String> = segment[shell::command(&segment, PREFIXES)..].to_vec();
        if words.is_empty() || matches!(shell::basename(&words[0]), "echo" | "printf") {
            continue;
        }
        if depth < DEPTH && SHELLS.contains(&shell::basename(&words[0])) {
            let script = words.iter().skip(1).position(|w| w.starts_with('-') && !w.starts_with("--") && w.contains('c')).and_then(|at| words.get(at + 2));
            if let Some(script) = script {
                out.extend(runs(script, depth + 1));
            }
        }
        if depth < DEPTH {
            for script in words.windows(2).filter(|pair| pair[0] == "--run").map(|pair| &pair[1]) {
                out.extend(runs(script, depth + 1));
            }
        }
        out.push(words);
    }
    out
}

fn assignment(word: &str) -> bool {
    !word.starts_with('-') && word.split_once('=').is_some_and(|(name, _)| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

fn launch_positions(words: &[String]) -> impl Iterator<Item = usize> + '_ {
    (0..words.len()).filter(|&at| at == 0 || LAUNCH_MARKERS.contains(&words[at - 1].as_str()) || assignment(&words[at - 1]))
}

fn launches(words: &[String], spec: &str) -> bool {
    let spec: Vec<&str> = spec.split_whitespace().collect();
    launch_positions(words).any(|at| matches_at(words, at, &spec))
}

fn matches_at(words: &[String], at: usize, spec: &[&str]) -> bool {
    let Some((head, rest)) = spec.split_first() else {
        return false;
    };
    if shell::basename(&words[at]) != *head {
        return false;
    }
    let mut cursor = at + 1;
    rest.iter().all(|want| {
        if want.starts_with('-') {
            return words[at + 1..].iter().any(|word| word == want);
        }
        while let Some(word) = words.get(cursor) {
            cursor += 1;
            if VALUE_FLAGS.contains(&word.as_str()) {
                cursor += 1;
            } else if !word.starts_with('-') {
                return word == want || (want.ends_with('#') && word.starts_with(want));
            }
        }
        false
    })
}

fn dev_shell_flake(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors().map(|dir| dir.join("flake.nix")).find(|flake| flake.is_file()).filter(|flake| std::fs::read_to_string(flake).is_ok_and(|text| text.contains("devShell")))
}

fn refuses(deny: &Deny, runs: &[Vec<String>]) -> bool {
    runs.iter().any(|words| {
        let wrapped = launch_positions(words).any(|at| deny.wrappers.iter().any(|wrapper| shell::basename(&words[at]) == wrapper) && deny.inner.iter().any(|inner| find(&words[at + 1..], inner)));
        wrapped || deny.commands.iter().any(|spec| launches(words, spec))
    })
}

fn find(words: &[String], spec: &str) -> bool {
    let spec: Vec<&str> = spec.split_whitespace().collect();
    (0..words.len()).any(|at| matches_at(words, at, &spec))
}

