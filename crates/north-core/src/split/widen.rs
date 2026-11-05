use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::scan::{declared_module, rust_files};
use super::{Plan, cargo};

const CODES: &[&str] = &["E0603", "E0624", "E0616", "E0432", "E0433", "E0425", "E0412", "E0422", "E0423", "E0531", "private_interfaces"];
const ROUNDS: usize = 12;
const ITEMS: &[&str] = &["async ", "const ", "unsafe ", "fn ", "struct ", "enum ", "type ", "trait ", "static ", "mod ", "use "];

#[derive(Default)]
pub struct Widened {
    pub widened: Vec<String>,
    pub test_items: Vec<String>,
}

pub fn until_clean(plan: &Plan) -> Result<Widened, String> {
    let mut result = Widened::default();
    for _ in 0..ROUNDS {
        let (success, diagnostics) = cargo::check(plan)?;
        if success {
            return Ok(result);
        }
        let sites = sites(plan, &diagnostics);
        let gated = gated(plan, &sites)?;
        let mut changed = false;
        for (file, line) in &sites {
            changed |= edit(plan, file, *line, &gated, &mut result)?;
        }
        if !changed {
            return Err(format!("cargo check -p {} fails with nothing left to widen:\n{}", plan.to, rendered(&diagnostics)));
        }
    }
    Err(format!("cargo check -p {} still fails after {ROUNDS} widening rounds", plan.to))
}

fn rendered(diagnostics: &[Value]) -> String {
    diagnostics.iter().filter(|message| message["level"] == "error").filter_map(|message| message["rendered"].as_str()).take(8).collect::<Vec<_>>().join("\n")
}

fn editable(plan: &Plan, file: &str) -> Option<String> {
    let file = Path::new(file).strip_prefix(&plan.root).map_or_else(|_| file.to_string(), |relative| relative.to_string_lossy().into_owned());
    (file.starts_with("crates/") && !file.starts_with(&format!("crates/{}/", plan.to))).then_some(file)
}

fn sites(plan: &Plan, diagnostics: &[Value]) -> BTreeSet<(String, usize)> {
    let mut sites = BTreeSet::new();
    for message in diagnostics {
        let code = message["code"]["code"].as_str().unwrap_or_default();
        if !CODES.contains(&code) {
            continue;
        }
        let children = message["children"].as_array().into_iter().flatten();
        let spans = message["spans"].as_array().into_iter().flatten().chain(children.flat_map(|child| child["spans"].as_array().into_iter().flatten()));
        for span in spans {
            let line = span["line_start"].as_u64().unwrap_or_default() as usize;
            if let Some(file) = span["file_name"].as_str().and_then(|file| editable(plan, file)) {
                sites.insert((file, line));
            }
        }
        if code == "E0616" {
            sites.extend(field(plan, message["message"].as_str().unwrap_or_default()));
        }
    }
    sites
}

fn field(plan: &Plan, message: &str) -> Option<(String, usize)> {
    let mut quoted = message.split('`').skip(1).step_by(2);
    let field = quoted.next()?;
    let owner = quoted.next()?.rsplit("::").next()?.split('<').next()?.to_string();
    for path in rust_files(&plan.root.join("crates")) {
        let relative = editable(plan, &path.to_string_lossy())?;
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let mut inside = false;
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            inside |= trimmed.contains(&format!("struct {owner} {{")) || trimmed.contains(&format!("struct {owner}<"));
            if inside && trimmed == "}" {
                inside = false;
            }
            let bare = trimmed.split_once(") ").filter(|(head, _)| head.starts_with("pub(")).map_or(trimmed, |(_, rest)| rest);
            if inside && bare.starts_with(&format!("{field}:")) {
                return Some((relative, index + 1));
            }
        }
    }
    None
}

fn line_at(plan: &Plan, file: &str, line: usize) -> Option<String> {
    fs::read_to_string(plan.root.join(file)).ok()?.lines().nth(line.checked_sub(1)?).map(str::to_string)
}

fn module_path(file: &str) -> String {
    let inner = file.splitn(4, '/').nth(3).unwrap_or(file).trim_end_matches(".rs");
    let inner = inner.strip_suffix("/mod").unwrap_or(inner);
    if inner == "lib" || inner == "mod" { String::new() } else { inner.replace('/', "::") }
}

fn gated(plan: &Plan, sites: &BTreeSet<(String, usize)>) -> Result<BTreeSet<(String, usize)>, String> {
    let mut allowed = BTreeSet::new();
    let mut refused = Vec::new();
    for (file, line) in sites {
        if line_at(plan, file, *line).is_none_or(|text| text.trim() != "#[cfg(test)]") {
            continue;
        }
        let item = item_after(plan, file, *line);
        let parent = module_path(file);
        let name = item.as_deref().and_then(declared_module).map(|name| if parent.is_empty() { name.to_string() } else { format!("{parent}::{name}") });
        match name {
            Some(name) if plan.allow_test_items.iter().any(|allow| *allow == name || name.ends_with(&format!("::{allow}"))) => {
                allowed.insert((file.clone(), *line));
            }
            Some(name) => refused.push(format!("{file}:{line} #[cfg(test)] mod {name}; pass --allow-test-item {name} to make it a #[doc(hidden)] pub mod, or move it beside the moved tests")),
            None => refused.push(format!("{file}:{line} #[cfg(test)] {}; only a #[cfg(test)] mod can become a #[doc(hidden)] pub mod, so wrap it in one or move it beside the moved tests", item.unwrap_or_default().trim())),
        }
    }
    if refused.is_empty() {
        return Ok(allowed);
    }
    Err(format!("refusing to widen a #[cfg(test)] item into production:\n  {}", refused.join("\n  ")))
}

fn item_after(plan: &Plan, file: &str, line: usize) -> Option<String> {
    let text = fs::read_to_string(plan.root.join(file)).ok()?;
    text.lines().skip(line).find(|next| !next.trim_start().starts_with("#[")).map(str::to_string)
}

fn edit(plan: &Plan, file: &str, line: usize, gated: &BTreeSet<(String, usize)>, result: &mut Widened) -> Result<bool, String> {
    let path = plan.root.join(file);
    let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let Some(current) = line.checked_sub(1).and_then(|index| lines.get(index)).cloned() else { return Ok(false) };
    let indent = &current[..current.len() - current.trim_start().len()];
    let next = if gated.contains(&(file.to_string(), line)) {
        let item = item_after(plan, file, line).unwrap_or_default();
        result.test_items.push(format!("{file}:{line} {}", item.trim()));
        format!("{indent}#[doc(hidden)]")
    } else {
        widened(&current)
    };
    if next == current {
        return Ok(false);
    }
    if !gated.contains(&(file.to_string(), line)) {
        result.widened.push(format!("{file}:{line} {}", next.trim()));
    }
    lines[line - 1] = next;
    let ending = if text.ends_with('\n') { "\n" } else { "" };
    fs::write(&path, format!("{}{ending}", lines.join("\n"))).map_err(|error| error.to_string())?;
    Ok(true)
}

fn widened(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    if let Some(rest) = trimmed.strip_prefix("pub(").and_then(|rest| rest.split_once(") ")).map(|(_, rest)| rest) {
        return format!("{indent}pub {rest}");
    }
    if trimmed.starts_with("pub ") || trimmed.starts_with("#[") {
        return line.to_string();
    }
    let field = trimmed.split_once(':').is_some_and(|(name, rest)| !rest.starts_with(':') && name.chars().all(|c| c.is_alphanumeric() || c == '_'));
    if ITEMS.iter().any(|item| trimmed.starts_with(item)) || field {
        return format!("{indent}pub {trimmed}");
    }
    line.to_string()
}
