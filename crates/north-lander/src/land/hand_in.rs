//! `north hand-in`: the one call a session hands its lane in with (north#1158). It stages and commits exactly the named paths in the lane that holds them, refusing a path in another checkout or paths with no changes before anything is committed, then hands the lane to the same road `safe-push --to main --wait` takes, so every gate and the lander's verdict come back unchanged.

use std::path::{Component, Path, PathBuf};

use super::enqueue::{default_branch, git};

pub const USAGE: &str = "usage: north hand-in --message TEXT [--] PATH...";

pub struct Lane {
    pub root: PathBuf,
    pub destination: String,
}

struct Request<'a> {
    message: &'a str,
    paths: Vec<PathBuf>,
}

pub fn run(arguments: &[String], road: impl FnOnce(&Lane) -> u8) -> u8 {
    let request = match parse(arguments) {
        Ok(request) => request,
        Err(code) => return code,
    };
    let committed = std::env::current_dir().map_err(|error| format!("current directory: {error}")).and_then(|directory| commit(&directory, &request));
    match committed {
        Ok(lane) => road(&lane),
        Err(refusal) => {
            eprintln!("hand-in: ✗ {refusal}; NOT committed.");
            1
        }
    }
}

fn parse(arguments: &[String]) -> Result<Request<'_>, u8> {
    let (message, rest) = match arguments {
        [flag, message, rest @ ..] if flag == "--message" => (message.as_str(), rest),
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            return Err(0);
        }
        _ => {
            eprintln!("{USAGE}");
            return Err(64);
        }
    };
    let rest = rest.strip_prefix(&["--".to_owned()][..]).unwrap_or(rest);
    Ok(Request { message, paths: rest.iter().map(PathBuf::from).collect() })
}

fn commit(directory: &Path, request: &Request) -> Result<Lane, String> {
    if request.message.trim().is_empty() {
        return Err("the message is empty".to_owned());
    }
    let paths: Vec<PathBuf> = request.paths.iter().map(|path| normal(&directory.join(path))).collect();
    let Some(first) = paths.first() else { return Err("name the paths to hand in".to_owned()) };
    let root = checkout(first).ok_or_else(|| format!("{} is in no git checkout", first.display()))?;
    if let Some(outside) = paths.iter().find(|path| checkout(path).as_ref() != Some(&root)) {
        return Err(format!("{} is outside the lane {}", outside.display(), root.display()));
    }
    let destination = default_branch(&root);
    match git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(branch) if branch != destination => {}
        _ => return Err(format!("{} is not on a lane's own branch", root.display())),
    }
    let named: Vec<&str> = paths.iter().filter_map(|path| path.to_str()).collect();
    if named.len() != paths.len() {
        return Err("a path is not valid UTF-8".to_owned());
    }
    git(&root, &[&["add", "--all", "--"][..], &named].concat())?;
    if git(&root, &[&["diff", "--cached", "--name-only", "HEAD", "--"][..], &named].concat())?.is_empty() {
        return Err(format!("no changes in {}", named.join(" ")));
    }
    git(&root, &[&["commit", "--quiet", "--only", "--message", request.message, "--"][..], &named].concat())?;
    Ok(Lane { root, destination })
}

fn checkout(path: &Path) -> Option<PathBuf> {
    let existing = path.ancestors().find(|ancestor| ancestor.is_dir())?;
    git(existing, &["rev-parse", "--show-toplevel"]).ok().map(PathBuf::from)
}

fn normal(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}
