use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

const USAGE: &str = "usage: north landed-tree [--repo DIR] BASE TIP";
const PATH_CHUNK: usize = 400;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Verdict {
    TreeOnMain,
    SupersededOnMain,
}

impl Verdict {
    fn label(self) -> &'static str {
        match self {
            Verdict::TreeOnMain => "tree already on main",
            Verdict::SupersededOnMain => "superseded on main",
        }
    }
}

#[derive(Default)]
struct Lines {
    added: Vec<String>,
    removed: Vec<String>,
}

fn meaningful(line: &&String) -> bool {
    line.chars().any(|c| c.is_alphanumeric() || c == '_')
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(repo).args(["-c", "core.quotePath=false", "--literal-pathspecs"]).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8(output.stdout).ok()).flatten()
}

fn nul_paths(output: &str) -> Vec<String> {
    output.split('\0').filter(|p| !p.is_empty()).map(str::to_owned).collect()
}

fn parse_patch(patch: &str) -> BTreeMap<String, Lines> {
    let mut files: BTreeMap<String, Lines> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut in_header = false;
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let half = rest.len().saturating_sub(5) / 2;
            let path = rest.get(2..2 + half).unwrap_or_default().to_owned();
            files.entry(path.clone()).or_default();
            current = Some(path);
            in_header = true;
        } else if line.starts_with("@@") {
            in_header = false;
        } else if let (Some(path), false) = (&current, in_header) {
            let entry = files.entry(path.clone()).or_default();
            if let Some(text) = line.strip_prefix('+') {
                entry.added.push(text.trim().to_owned());
            } else if let Some(text) = line.strip_prefix('-') {
                entry.removed.push(text.trim().to_owned());
            }
        }
    }
    files
}

pub(crate) fn landed(repo: &Path, base: &str, tip: &str) -> Option<Verdict> {
    let merge_base = git(repo, &["merge-base", base, tip])?.trim().to_owned();
    let changed = nul_paths(&git(repo, &["diff", "--name-only", "-z", "--no-renames", &merge_base, tip])?);
    if changed.is_empty() {
        return Some(Verdict::TreeOnMain);
    }
    let mut open = Vec::new();
    for part in changed.chunks(PATH_CHUNK) {
        let mut args = vec!["diff", "--name-only", "-z", "--no-renames", base, tip, "--"];
        args.extend(part.iter().map(String::as_str));
        open.extend(nul_paths(&git(repo, &args)?));
    }
    if open.is_empty() {
        return Some(Verdict::TreeOnMain);
    }
    let range = format!("{merge_base}..{base}");
    let mut branch = BTreeMap::new();
    let mut main = BTreeMap::new();
    for part in open.chunks(PATH_CHUNK) {
        let paths: Vec<&str> = part.iter().map(String::as_str).collect();
        let with_paths = |head: &[&'static str], middle: &[&str]| -> Vec<String> {
            head.iter().map(|s| s.to_string()).chain(middle.iter().map(|s| s.to_string())).chain(["--".to_owned()]).chain(paths.iter().map(|s| s.to_string())).collect()
        };
        let run = |args: Vec<String>| git(repo, &args.iter().map(String::as_str).collect::<Vec<_>>()).unwrap_or_default();
        branch.extend(parse_patch(&run(with_paths(&["diff", "-U0", "--no-color", "--no-renames"], &[&merge_base, tip]))));
        main.extend(parse_patch(&run(with_paths(&["log", "-p", "-U0", "--no-color", "--no-renames", "--format="], &[&range]))));
    }
    for path in &open {
        let mine = branch.get(path)?;
        if !(mine.added.iter().any(|l| meaningful(&l)) || mine.removed.iter().any(|l| meaningful(&l))) {
            return None;
        }
        let spec = format!("{base}:{path}");
        let text = match git(repo, &["show", &spec]) {
            Some(text) => text,
            None if git(repo, &["cat-file", "-e", &spec]).is_none() => String::new(),
            None => return None,
        };
        let current: BTreeSet<&str> = text.lines().map(str::trim).collect();
        let later: BTreeSet<&str> = main.get(path).map(|m| m.added.iter().map(String::as_str).collect()).unwrap_or_default();
        if !mine.added.iter().filter(meaningful).all(|l| later.contains(l.as_str())) {
            return None;
        }
        if !mine.removed.iter().filter(meaningful).all(|l| !current.contains(l.as_str()) || later.contains(l.as_str())) {
            return None;
        }
    }
    let on_main = commits(&git(repo, &["log", "--format=%x01%s", "--name-only", "--no-renames", &range])?);
    let lane = commits(&git(repo, &["log", "--format=%x01%s", "--name-only", "--no-renames", &format!("{merge_base}..{tip}")])?);
    let carried = |(subject, files): &(String, BTreeSet<String>)| on_main.iter().any(|(s, f)| s == subject && f.is_superset(files));
    lane.iter().all(carried).then_some(Verdict::SupersededOnMain)
}

fn commits(log: &str) -> Vec<(String, BTreeSet<String>)> {
    log.split('\x01').filter(|c| !c.trim().is_empty()).map(|c| {
        let mut lines = c.lines();
        (lines.next().unwrap_or_default().to_owned(), lines.filter(|l| !l.is_empty()).map(str::to_owned).collect())
    }).collect()
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let (repo, rest) = match arguments {
        [flag, dir, rest @ ..] if flag == "--repo" => (Path::new(dir), rest),
        rest => (Path::new("."), rest),
    };
    let [base, tip] = rest else {
        eprintln!("{USAGE}");
        return 64;
    };
    match landed(repo, base, tip) {
        Some(verdict) => {
            println!("{}", verdict.label());
            0
        }
        None => 1,
    }
}

#[cfg(test)]
#[path = "landed_tree_tests.rs"]
mod tests;
