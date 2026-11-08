
pub const FEEDBACK_LOOP: &str = "docs/feedback-loop.md";
pub const LOOPS: &str = "harness/loops.toml";

const ROOTS: &[&str] = &["bin/", "crates/", "harness/", "src/", "docs/", "lib/"];

pub trait Tree {
    fn exists(&self, path: &str) -> bool;
    fn text(&self, path: &str) -> Option<String>;
}

fn signal_rows(doc: &str) -> Vec<Vec<&str>> {
    let mut rows = Vec::new();
    let mut inside = false;
    for line in doc.lines() {
        if line.starts_with("| signal | writer | reader on main") {
            inside = true;
            continue;
        }
        if inside && !line.starts_with('|') {
            break;
        }
        if inside && !line.starts_with("|---") {
            rows.push(line.trim().trim_matches('|').split('|').map(str::trim).collect());
        }
    }
    rows
}

enum Cite {
    Path(String, Option<usize>),
    Line(usize),
    Symbol(String),
}

fn classify(raw: &str) -> Option<Cite> {
    let token = raw.trim_matches(|c: char| matches!(c, ',' | ';' | '.' | '`' | '(' | ')'));
    if let Some(line) = token.strip_prefix(':') {
        return line.parse().ok().map(Cite::Line);
    }
    let token = token.trim_end_matches(':');
    if ROOTS.iter().any(|root| token.starts_with(root)) {
        return Some(match token.rsplit_once(':') {
            Some((path, line)) if line.parse::<usize>().is_ok() => Cite::Path(path.into(), line.parse().ok()),
            _ => Cite::Path(token.into(), None),
        });
    }
    let word = !token.is_empty() && token.contains('_') && token.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    word.then(|| Cite::Symbol(token.into()))
}

fn resolve(tree: &impl Tree, path: &str, line: Option<usize>, symbol: Option<&str>) -> Result<(), String> {
    if !tree.exists(path) {
        return Err(format!("{path} does not exist"));
    }
    if let Some(line) = line {
        let length = tree.text(path).map_or(0, |text| text.lines().count());
        if line > length {
            return Err(format!("{path}:{line} is past the end ({length} lines)"));
        }
    }
    if let Some(symbol) = symbol
        && !tree.text(path).is_some_and(|text| text.contains(symbol))
    {
        return Err(format!("{symbol} is not in {path}"));
    }
    Ok(())
}

fn check_cell(tree: &impl Tree, cell: &str) -> Vec<String> {
    let mut faults = Vec::new();
    let mut current: Option<String> = None;
    let mut fresh = false;
    for word in cell.split_whitespace() {
        match classify(word) {
            Some(Cite::Path(path, line)) => {
                faults.extend(resolve(tree, &path, line, None).err());
                current = Some(path);
                fresh = true;
            }
            Some(Cite::Line(line)) => {
                if let Some(path) = &current {
                    faults.extend(resolve(tree, path, Some(line), None).err());
                    fresh = true;
                }
            }
            Some(Cite::Symbol(symbol)) => {
                if let (true, Some(path)) = (fresh, &current) {
                    faults.extend(resolve(tree, path, None, Some(&symbol)).err());
                }
                fresh = false;
            }
            None => fresh = false,
        }
    }
    faults.dedup();
    faults
}

pub fn dead_readers(doc: &str, tree: &impl Tree) -> Vec<String> {
    signal_rows(doc)
        .into_iter()
        .filter(|cells| cells.len() >= 3)
        .flat_map(|cells| {
            let name = cells[0].to_owned();
            check_cell(tree, cells[2]).into_iter().map(move |fault| format!("{name}: {fault}"))
        })
        .collect()
}

pub fn dead_loop_sites(registry: &str, tree: &impl Tree) -> Vec<String> {
    let Ok(document) = registry.parse::<toml_edit::DocumentMut>() else {
        return vec![format!("{LOOPS} does not parse")];
    };
    let Some(loops) = document.get("loop").and_then(toml_edit::Item::as_array_of_tables) else {
        return Vec::new();
    };
    let mut faults = Vec::new();
    for entry in loops {
        let name = entry.get("name").and_then(toml_edit::Item::as_str).unwrap_or("?");
        for key in ["sites", "constants"] {
            let cited = entry.get(key).and_then(toml_edit::Item::as_array).into_iter().flatten().filter_map(toml_edit::Value::as_str);
            for cite in cited {
                let (path, constant) = cite.split_once("::").map_or((cite, None), |(path, constant)| (path, Some(constant)));
                if let Err(fault) = resolve(tree, path, None, constant) {
                    faults.push(format!("loop {name}: {fault}"));
                }
            }
        }
    }
    faults
}
