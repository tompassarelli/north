use serde_json::{Value, json};

use super::parts::{Part, Split};

pub struct Named<'s> {
    pub id: &'s str,
    pub base: &'s str,
    pub check: &'s str,
}

pub fn stub(id: &str) -> String {
    format!("split-{id}")
}

pub fn trailer(id: &str, which: &str) -> String {
    format!("Split-box: {id} {which}")
}

fn listed<'a>(items: impl IntoIterator<Item = &'a String>) -> String {
    items.into_iter().map(|item| format!("`{item}`")).collect::<Vec<_>>().join(", ")
}

fn check(crates: &[&str]) -> String {
    if crates.is_empty() {
        return "cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace".to_owned();
    }
    let packages: String = crates.iter().map(|name| format!(" -p {name}")).collect();
    format!("cargo clippy{packages} --all-targets -- -D warnings && cargo nextest run{packages}")
}

fn first(split: &Split, named: &Named) -> String {
    let shared = &split.shared;
    let stub = stub(named.id);
    let mut text = format!("- [ ] Split {}, first box (lands alone, before every part): lay the skeleton and nothing else. Files: {}.", named.id, listed(&shared.files));
    if !shared.modules.is_empty() {
        text.push_str(&format!(" Create as empty modules declared in their parents: {}.", listed(&shared.modules)));
    }
    if !shared.dependencies.is_empty() {
        let edges: Vec<String> = shared.dependencies.iter().map(|(user, owner)| format!("{user} on {owner}")).collect();
        text.push_str(&format!(" Add the crate dependencies {}.", listed(&edges)));
    }
    if !shared.declarations.is_empty() {
        let declared: Vec<String> = shared.declarations.iter().map(|declaration| format!("{} in {}", declaration.signature, declaration.path)).collect();
        text.push_str(&format!(" Declare {}, each function body `todo!(\"{stub}\")` and each item the repository would warn about as unused under `#[expect(dead_code, reason = \"{stub}\")]`.", listed(&declared)));
    }
    text.push_str(&format!(" It passes the repository's checks alone, warnings included, and its commit message carries the line `{}`. Check: `{}`.", trailer(named.id, "first"), check(&[])));
    text
}

fn part(index: usize, count: usize, part: &Part, named: &Named) -> String {
    let crates: Vec<&str> = part.crates.iter().map(String::as_str).collect();
    let summary = part.summaries.iter().filter(|summary| !summary.is_empty()).cloned().collect::<Vec<_>>().join(" ");
    format!(
        "- [ ] Split {}, part {index} of {count}, {} (after the first box, beside the other parts): {summary} Files, its write set, which no other part names: {}. It removes every `{}` stub its files hold, and its commit message carries the line `{}`. Check: `{}`.",
        named.id,
        part.names.join(" + "),
        listed(&part.files),
        stub(named.id),
        trailer(named.id, &format!("part {index}")),
        check(&crates)
    )
}

pub fn markdown(split: &Split, named: &Named) -> String {
    let count = split.parts.len();
    let mut lines = vec![format!("Split {} of the task at base {}: a first box, {count} part(s) that run at once, and a last box.", named.id, named.base), String::new(), first(split, named)];
    lines.extend(split.parts.iter().enumerate().map(|(index, held)| part(index + 1, count, held, named)));
    lines.push(format!(
        "- [ ] Split {}, last box (after every part): every part has landed and no `{}` stub remains on main; to abandon the task instead, revert the first box's commit, which the check prints and records. Check: `{}`.",
        named.id,
        stub(named.id),
        named.check
    ));
    lines.join("\n")
}

pub fn recorded(split: &Split) -> Value {
    let shared = &split.shared;
    let declarations: Vec<Value> = shared.declarations.iter().map(|declaration| json!({ "symbol": declaration.symbol.shown(), "path": declaration.path, "signature": declaration.signature })).collect();
    let dependencies: Vec<Value> = shared.dependencies.iter().map(|(user, owner)| json!({ "crate": user, "on": owner })).collect();
    let parts: Vec<Value> = split.parts.iter().map(|part| json!({ "names": part.names, "files": part.files, "crates": part.crates })).collect();
    json!({ "first": { "files": shared.files, "modules": shared.modules, "dependencies": dependencies, "declarations": declarations }, "parts": parts })
}
