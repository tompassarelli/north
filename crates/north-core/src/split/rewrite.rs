use std::fs;
use std::path::Path;

use super::scan::{self, paths_after};
use super::{Module, Plan, git};

fn ident(package: &str) -> String {
    package.replace('-', "_")
}

fn tracked(plan: &Plan) -> Result<Vec<String>, String> {
    Ok(git(&plan.root, &["ls-files"])?.lines().map(str::to_string).collect())
}

fn reaches(paths: &[String], module: &str) -> bool {
    paths.iter().any(|path| path == module || path.starts_with(&format!("{module}::")))
}

fn owned_by(module: &Module, file: &str) -> bool {
    [&module.old_dir, &module.old_file, &module.old_tests].into_iter().flatten().any(|old| file == old || file.starts_with(&format!("{old}/")))
}

pub fn refuse_back_edges(plan: &Plan, pending: &[Module]) -> Result<(), String> {
    let from_src = format!("crates/{}/src/", plan.from);
    let from_ident = ident(&plan.from);
    let mut edges = Vec::new();
    for file in tracked(plan)?.iter().filter(|file| file.ends_with(".rs") && file.starts_with("crates/")) {
        if pending.iter().any(|module| owned_by(module, file)) || file.starts_with(&format!("crates/{}/", plan.to)) {
            continue;
        }
        let Ok(text) = fs::read_to_string(plan.root.join(file)).map(|text| scan::code_only(&text)) else { continue };
        let mut paths = paths_after(&text, "crate");
        if !file.starts_with(&from_src) {
            paths.extend(paths_after(&text, &from_ident));
        }
        for module in pending.iter().filter(|module| reaches(&paths, &module.rust_path())) {
            edges.push(format!("{file} names {}", module.rust_path()));
        }
    }
    if edges.is_empty() {
        return Ok(());
    }
    Err(format!("these files still reach the modules being moved; cut the edges first:\n  {}", edges.join("\n  ")))
}

pub fn drop_declaration(plan: &Plan, module: &Module) -> Result<(), String> {
    let parent = match module.path.rsplit_once('/') {
        Some((parent, _)) => format!("crates/{}/src/{parent}", plan.from),
        None => format!("crates/{}/src/lib", plan.from),
    };
    let candidates = [format!("{parent}/mod.rs"), format!("{parent}.rs")];
    let file = candidates.iter().map(|file| plan.root.join(file)).find(|file| file.exists()).ok_or_else(|| format!("no parent module file for {}", module.path))?;
    let text = fs::read_to_string(&file).map_err(|error| error.to_string())?;
    let kept = text.lines().filter(|line| scan::declared_module(line) != Some(module.name.as_str())).map(|line| format!("{line}\n")).collect::<String>();
    fs::write(&file, kept).map_err(|error| error.to_string())
}

pub fn workspace(plan: &Plan) -> Result<(), String> {
    let path = plan.root.join("Cargo.toml");
    let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let member = format!("\"crates/{}\"", plan.to);
    let dependency = format!("{} = {{ path = \"crates/{}\" }}", plan.to, plan.to);
    let from_line = format!("{} = {{ path = \"crates/{}\" }}", plan.from, plan.from);
    let has_package = text.lines().any(|line| line.trim() == "[package]");
    let has_dependency = text.lines().any(|line| line.starts_with(&format!("{} = ", plan.to)));
    let mut out = String::new();
    let mut placed = has_dependency || !has_package;
    for line in text.lines() {
        let array = line.starts_with("members = [") || line.starts_with("default-members = [");
        if array && !line.contains(&member) && line.trim_end().ends_with(']') {
            out.push_str(&format!("{}, {member}]\n", line.trim_end().trim_end_matches(']')));
            continue;
        }
        out.push_str(line);
        out.push('\n');
        if !placed && (line == from_line || (line.trim() == "[dependencies]" && !text.contains(&from_line))) {
            out.push_str(&format!("{dependency}\n"));
            placed = true;
        }
    }
    fs::write(&path, out).map_err(|error| error.to_string())
}

pub fn callers(plan: &Plan, pending: &[Module]) -> Result<(), String> {
    let (from, to) = (ident(&plan.from), ident(&plan.to));
    let mut imported = Vec::new();
    for file in tracked(plan)?.iter().filter(|file| file.ends_with(".rs") && !file.starts_with(&format!("crates/{}/", plan.to))) {
        let path = plan.root.join(file);
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let mut next = text.clone();
        for module in pending {
            let rust = module.rust_path();
            next = replace_path(&next, &format!("{from}::{rust}"), &format!("{to}::{}", module.name));
            if !file.starts_with("crates/") {
                let before = next.clone();
                next = remove_from_list(&next, &from, &rust);
                next = replace_path(&next, &format!("crate::{rust}"), &format!("crate::{}", module.name));
                if next != before || reaches(&paths_after(&next, "crate"), &module.name) {
                    imported.push(module.name.clone());
                }
            }
        }
        if next != text {
            fs::write(&path, next).map_err(|error| error.to_string())?;
        }
    }
    imported.sort();
    imported.dedup();
    import(plan, &to, &imported)
}

fn replace_path(text: &str, old: &str, new: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(found) = rest.find(old) {
        let before = if found > 0 { rest[..found].chars().next_back() } else { out.chars().next_back() };
        let after = rest[found + old.len()..].chars().next();
        let bounded = !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':') && !after.is_some_and(|c| c.is_alphanumeric() || c == '_');
        out.push_str(&rest[..found]);
        out.push_str(if bounded { new } else { old });
        rest = &rest[found + old.len()..];
    }
    out.push_str(rest);
    out
}

fn remove_from_list(text: &str, from: &str, module: &str) -> String {
    let opener = format!("use {from}::{{");
    let Some(start) = text.find(&opener) else { return text.to_string() };
    let body_start = start + opener.len();
    let Some(length) = text[body_start..].find('}') else { return text.to_string() };
    let body = &text[body_start..body_start + length];
    let items = body.split(',').map(str::trim).filter(|item| !item.is_empty()).collect::<Vec<_>>();
    if !items.contains(&module) {
        return text.to_string();
    }
    let edited = drop_item(body, module);
    format!("{}{edited}{}", &text[..body_start], &text[body_start + length..])
}

fn drop_item(body: &str, item: &str) -> String {
    let parts = body.split_inclusive(',').collect::<Vec<_>>();
    let index = parts.iter().position(|part| part.trim().trim_end_matches(',').trim() == item).unwrap_or(parts.len());
    let mut kept = parts.iter().enumerate().filter(|(at, _)| *at != index).map(|(_, part)| *part).collect::<String>();
    if index == parts.len() - 1 {
        kept = kept.trim_end_matches(|c: char| c == ',' || c.is_whitespace()).to_string();
        kept.push_str(&body[body.trim_end().len()..]);
    } else if index == 0 {
        let lead = &body[..body.len() - body.trim_start().len()];
        kept = format!("{lead}{}", kept.trim_start());
    }
    kept
}

fn import(plan: &Plan, to: &str, names: &[String]) -> Result<(), String> {
    let main = plan.root.join("src/main.rs");
    if names.is_empty() || !main.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(&main).map_err(|error| error.to_string())?;
    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let prefix = format!("use {to}::");
    let existing = lines.iter().position(|line| line.starts_with(&prefix));
    let mut all = existing.map(|at| paths_after(&lines[at], to)).unwrap_or_default();
    all.extend(names.iter().cloned());
    all.sort();
    all.dedup();
    let statement = if all.len() == 1 { format!("{prefix}{};", all[0]) } else { format!("{prefix}{{{}}};", all.join(", ")) };
    match existing {
        Some(at) => lines[at] = statement,
        None => {
            let last = last_use_end(&lines).ok_or("src/main.rs has no `use north_` statement to import beside")?;
            lines.insert(last + 1, statement);
        }
    }
    fs::write(&main, format!("{}\n", lines.join("\n"))).map_err(|error| error.to_string())
}

fn last_use_end(lines: &[String]) -> Option<usize> {
    let start = lines.iter().rposition(|line| line.starts_with("use north_"))?;
    lines.iter().skip(start).position(|line| line.trim_end().ends_with(';')).map(|offset| start + offset)
}

pub fn path_strings(plan: &Plan, pending: &[Module]) -> Result<(), String> {
    let mut pairs = Vec::new();
    for module in pending {
        pairs.push((format!("crates/{}/src/{}", plan.from, module.path), format!("crates/{}/src/{}", plan.to, module.name)));
        if module.old_tests.is_some() {
            pairs.push((format!("crates/{}/src/{}_tests.rs", plan.from, module.path), format!("crates/{}/src/{}_tests.rs", plan.to, module.name)));
        }
    }
    let moved = pending.iter().map(|module| format!("crates/{}/src/{}", plan.to, module.name)).collect::<Vec<_>>();
    for file in tracked(plan)? {
        if keeps_strings(&file) || moved.iter().any(|dest| file == format!("{dest}.rs") || file == format!("{dest}_tests.rs") || file.starts_with(&format!("{dest}/"))) {
            continue;
        }
        rewrite_file(&plan.root.join(&file), &pairs)?;
    }
    Ok(())
}

fn keeps_strings(file: &str) -> bool {
    file == "Cargo.lock" || file.ends_with("_tests.rs") || file.split('/').any(|part| part == "fixtures")
}

fn rewrite_file(path: &Path, pairs: &[(String, String)]) -> Result<(), String> {
    let Ok(text) = fs::read_to_string(path) else { return Ok(()) };
    let next = pairs.iter().fold(text.clone(), |text, (old, new)| replace_mention(&text, old, new));
    if next != text {
        fs::write(path, next).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn replace_mention(text: &str, old: &str, new: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(found) = rest.find(old) {
        let after = rest[found + old.len()..].chars().next();
        let bounded = !after.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
        out.push_str(&rest[..found]);
        out.push_str(if bounded { new } else { old });
        rest = &rest[found + old.len()..];
    }
    out.push_str(rest);
    out
}
