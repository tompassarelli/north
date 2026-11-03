use std::fs;
use std::path::{Path, PathBuf};

const SOURCE_DIR: &str = "docs/features";
const INDEX_FILE: &str = "docs/features.md";
const HEADER_FILE: &str = "_header.md";
const TITLE_FILE: &str = "_title.md";
const USAGE: &str = "usage: north docs features-index [--check] [--root DIR]";

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn block(text: &str) -> String {
    format!("{}\n", text.trim_end_matches('\n'))
}

fn is_item(block: &str) -> bool {
    let line = block.trim_start();
    line.starts_with("- ") || line.starts_with("* ")
}

fn sorted(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|error| format!("{}: {error}", dir.display())))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

fn hidden(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_none_or(|name| name.starts_with('_') || name.starts_with('.'))
}

fn section(dir: &Path) -> Result<String, String> {
    let mut out = format!("{}\n", block(&read(&dir.join(TITLE_FILE))?));
    let mut previous: Option<String> = None;
    for path in sorted(dir)?.into_iter().filter(|path| path.extension().is_some_and(|extension| extension == "md") && !hidden(path)) {
        let entry = block(&read(&path)?);
        if previous.as_ref().is_some_and(|previous| !(is_item(previous) && is_item(&entry))) {
            out.push('\n');
        }
        out.push_str(&entry);
        previous = Some(entry);
    }
    Ok(out)
}

pub fn render(source: &Path) -> Result<String, String> {
    let mut parts = vec![block(&read(&source.join(HEADER_FILE))?)];
    for path in sorted(source)?.into_iter().filter(|path| path.is_dir() && !hidden(path)) {
        parts.push(section(&path)?);
    }
    Ok(parts.join("\n"))
}

pub fn generate(root: &Path) -> Result<(), String> {
    let text = render(&root.join(SOURCE_DIR))?;
    let index = root.join(INDEX_FILE);
    if fs::read_to_string(&index).is_ok_and(|current| current == text) {
        return Ok(());
    }
    fs::write(&index, text).map_err(|error| format!("{}: {error}", index.display()))
}

pub fn check(root: &Path) -> Result<(), String> {
    let text = render(&root.join(SOURCE_DIR))?;
    match fs::read_to_string(root.join(INDEX_FILE)) {
        Ok(current) if current == text => Ok(()),
        _ => Err(format!("{INDEX_FILE} is stale: run `north docs features-index` and commit the result")),
    }
}

fn find_root() -> Option<PathBuf> {
    std::env::current_dir().ok()?.ancestors().find(|dir| dir.join(SOURCE_DIR).is_dir()).map(Path::to_path_buf)
}

fn features_index(arguments: &[String]) -> Result<(), String> {
    let mut checking = false;
    let mut root = None;
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--check" => checking = true,
            "--root" => root = Some(PathBuf::from(rest.next().ok_or(USAGE)?)),
            _ => return Err(USAGE.to_string()),
        }
    }
    let root = root.or_else(find_root).ok_or_else(|| format!("no {SOURCE_DIR} directory found from the current directory"))?;
    if checking { check(&root) } else { generate(&root) }
}

pub fn run(arguments: &[String]) -> u8 {
    let result = match arguments.split_first() {
        Some((command, rest)) if command == "features-index" => features_index(rest),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            if message == USAGE { 2 } else { 1 }
        }
    }
}

#[cfg(test)]
mod tests;
