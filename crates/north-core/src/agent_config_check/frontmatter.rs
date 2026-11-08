use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use regex::Regex;

pub(super) fn markdown_in(directory: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md") && path.is_file())
        .collect();
    found.sort();
    found
}

pub(super) fn strict<'a>(files: impl IntoIterator<Item = &'a PathBuf>, shown_from: &Path) -> Vec<String> {
    files.into_iter().filter_map(|file| refusal(file, shown_from)).collect()
}

fn refusal(file: &Path, shown_from: &Path) -> Option<String> {
    let shown = file.strip_prefix(shown_from).unwrap_or(file).display();
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) => return Some(format!("{shown}: {error}")),
    };
    let head = text.strip_prefix("---\n")?;
    let Some(end) = head.find("\n---\n").or_else(|| head.strip_suffix("\n---").map(str::len)) else {
        return Some(format!("{shown}:1: frontmatter opens with --- and never closes"));
    };
    let (line, reason) = yaml_error(&head[..=end])?;
    Some(format!("{shown}:{}: frontmatter is not strict YAML: {reason}", line + 1))
}

fn yaml_error(frontmatter: &str) -> Option<(usize, String)> {
    let spawned = Command::new("yq").args(["-o", "json", ".", "-"]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return Some((0, format!("cannot run yq: {error}"))),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(frontmatter.as_bytes());
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(error) => return Some((0, format!("yq did not finish: {error}"))),
    };
    if output.status.success() {
        return None;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let reason = stderr.lines().find(|line| !line.trim().is_empty()).unwrap_or("yq refused it").trim();
    let located = Regex::new(r"yaml: line (\d+)(?:, column \d+)?: (.*)").expect("yq error pattern").captures(reason);
    Some(match located {
        Some(found) => (found[1].parse().unwrap_or(0), found[2].to_owned()),
        None => (0, reason.trim_start_matches("Error: ").to_owned()),
    })
}
