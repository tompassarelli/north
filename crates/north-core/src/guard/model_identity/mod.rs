mod identity;
mod script;

use crate::guard::paths::{basename, dirname, expanduser, join, normpath, realpath};
use crate::hook::{HookInput, Verdict};
use crate::platform::pyjson::Value;
use identity::{splitlines, text_invalid};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

const ID: &str = "north-hook";
const REASON: &str = "BLOCKED: a todo model field must name an exact concrete runtime model, not a selection or lineage placeholder. Record the exact concrete model from run or dispatch evidence (for example `gpt-6-sol`); if that evidence is unavailable, record the evidence gap outside the model field instead of inventing an identity.";

static PATCH_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\*\*\* (?:Add|Update) File:[\s\x1c-\x1f]*(.+?)[\s\x1c-\x1f]*$").unwrap()
});

pub struct Context {
    home: String,
    cwd: String,
    todo_root: String,
}

impl Context {
    pub fn new(home: &str, cwd: &str, todo_root: &str) -> Self {
        let root = expanduser(todo_root, home);
        Self {
            home: home.to_owned(),
            cwd: cwd.to_owned(),
            todo_root: realpath(Path::new(&root)).unwrap_or(root),
        }
    }

    fn from_env(cwd: &str) -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let root = std::env::var("TODO_ROOT").unwrap_or_else(|_| "~/code/todo".into());
        Self::new(&home, cwd, &root)
    }

    fn resolve(&self, value: &str) -> Option<String> {
        if value.is_empty() {
            return None;
        }
        let value = value.replace("${HOME}", &self.home).replace("$HOME", &self.home);
        let value = expanduser(&value, &self.home);
        let value = if value.starts_with('/') { value } else { join(&self.cwd, &value) };
        realpath(Path::new(&normpath(&value)))
    }

    fn is_todo_record(&self, value: &str) -> bool {
        self.resolve(value).is_some_and(|path| {
            dirname(&path) == self.todo_root && path.to_lowercase().ends_with(".md")
        })
    }

    fn record_invalid(&self, path: &str, text: &str) -> bool {
        self.is_todo_record(path) && text_invalid(text, basename(path))
    }
}

pub fn patch_invalid(context: &Context, patch: &str) -> bool {
    let mut current: Option<String> = None;
    let mut added: Vec<&str> = Vec::new();
    let flush = |current: &Option<String>, added: &[&str]| {
        current.as_deref().is_some_and(|path| context.record_invalid(path, &added.join("\n")))
    };
    for line in splitlines(patch) {
        if let Some(found) = PATCH_HEADER.captures(line) {
            if flush(&current, &added) {
                return true;
            }
            current = Some(found[1].to_owned());
            added.clear();
        } else if line.starts_with("*** ") {
            if flush(&current, &added) {
                return true;
            }
            current = None;
            added.clear();
        } else if current.is_some() && line.starts_with('+') {
            added.push(&line[1..]);
        }
    }
    flush(&current, &added)
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number { raw, .. } => raw.parse::<f64>() != Ok(0.0),
        Value::String(text) => !text.lossy().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(entries) => !entries.is_empty(),
    }
}

fn first<'a>(input: &'a Value, keys: [&str; 2]) -> &'a Value {
    let primary = &input[keys[0]];
    if truthy(primary) { primary } else { &input[keys[1]] }
}

fn edit_values<'a>(tool: &str, input: &'a Value) -> Option<Vec<&'a Value>> {
    match tool {
        "Write" => Some(vec![&input["content"]]),
        "Edit" => Some(vec![first(input, ["new_string", "newString"])]),
        _ => Some(
            input["edits"]
                .as_array()?
                .iter()
                .filter(|edit| edit.is_object())
                .map(|edit| first(edit, ["new_string", "newString"]))
                .collect(),
        ),
    }
}

fn edit_invalid(context: &Context, tool: &str, input: &Value) -> bool {
    let Some(path) = first(input, ["file_path", "filePath"]).as_str() else {
        return false;
    };
    if !context.is_todo_record(path) {
        return false;
    }
    edit_values(tool, input)
        .is_some_and(|values| values.iter().filter_map(|v| v.as_str()).any(|text| context.record_invalid(path, text)))
}

impl Context {
    pub fn decide(&self, input: &HookInput) -> bool {
        let data = &input.data;
        let tool = if truthy(&data["tool_name"]) { &data["tool_name"] } else { &data["toolName"] };
        let tool = tool.as_str().unwrap_or("").rsplit('.').next().unwrap_or("");
        let tool_input = if truthy(&data["tool_input"]) { &data["tool_input"] } else { &data["toolInput"] };
        if !tool_input.is_object() {
            return false;
        }
        match tool {
            "Edit" | "Write" | "MultiEdit" => edit_invalid(self, tool, tool_input),
            "apply_patch" => tool_input["input"].as_str().is_some_and(|patch| patch_invalid(self, patch)),
            _ if input.is_shell() => input
                .command()
                .is_some_and(|command| script::writes_invalid(self, &command.lossy(), 0)),
            _ => false,
        }
    }
}

pub fn evaluate(input: &HookInput) -> Verdict {
    let raw = String::from_utf8_lossy(&input.raw).to_lowercase();
    if !(raw.contains("model") || raw.contains("estimate-calibration")) {
        return Verdict::Allow;
    }
    if crate::guard::activation::authoring_guards_off(ID) {
        return Verdict::Allow;
    }
    let cwd = input
        .data["cwd"]
        .as_str()
        .filter(|cwd| !cwd.is_empty())
        .map(str::to_owned)
        .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_default();
    if Context::from_env(&cwd).decide(input) {
        Verdict::Deny(REASON.into())
    } else {
        Verdict::Allow
    }
}

#[cfg(test)]
mod tests;
