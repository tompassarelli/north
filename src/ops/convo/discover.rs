use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::env::{Env, canonical};
use super::failure::{Failure, Result};
use super::records::Kind;

static SESSION_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\.jsonl(?:\.zst)?$").expect("session file expression")
});

#[derive(Clone, Copy)]
pub(super) struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub mtime: (i64, i64),
    pub atime: (i64, i64),
}

impl Stat {
    pub(super) fn of(meta: &fs::Metadata) -> Self {
        Self {
            dev: meta.dev(),
            ino: meta.ino(),
            size: meta.size(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
            atime: (meta.atime(), meta.atime_nsec()),
        }
    }

    pub(super) fn mtime_secs(&self) -> f64 {
        self.mtime.0 as f64 + self.mtime.1 as f64 * 1e-9
    }
}

pub(super) struct Root {
    pub path: String,
    pub provider: String,
    pub account: String,
    pub layout: &'static str,
}

pub(super) struct Found {
    pub logical: String,
    pub disk: String,
    pub provider: String,
    pub account: String,
    pub hint: String,
    pub kind: Kind,
    pub stat: Stat,
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

type Candidate<'a> = (Option<String>, &'a str, &'a str, &'static str, bool, &'a str);

pub(super) fn transcript_roots(env: &Env) -> Result<Vec<Root>> {
    let local = |path: &str| env.home.join(path).to_string_lossy().into_owned();
    let candidates: Vec<Candidate> = if env.root_pinned {
        vec![(Some(display(&env.root)), "", "", "accounts", true, "CONVO_ROOT")]
    } else {
        vec![
            (Some(display(&env.root)), "", "", "accounts", false, "account transcript root"),
            (env.codex_home.clone(), "openai", "configured", "codex", env.codex_home.is_some(), "CODEX_HOME"),
            (env.pooled_home.clone(), "openai", "pooled", "codex", env.pooled_home.is_some(), "NORTH_CODEX_POOLED_HOME"),
            (Some(local(".local/state/north/codex-pooled")), "openai", "pooled", "codex", false, "default pooled transcript root"),
            (env.claude_config.as_ref().map(|dir| format!("{dir}/projects")), "anthropic", "local", "claude", env.claude_config.is_some(), "CLAUDE_CONFIG_DIR projects"),
            (Some(local(".claude/projects")), "anthropic", "local", "claude", false, "Claude Code projects root"),
        ]
    };
    let mut roots: Vec<Root> = Vec::new();
    let mut seen = HashSet::new();
    for (path, provider, account, layout, required, label) in candidates {
        let Some(path) = path.filter(|path| !path.is_empty()) else { continue };
        let canonical = canonical(&expand(&path, &env.home));
        if !canonical.is_dir() {
            if required {
                return Err(Failure::Unreconciled(format!("{label} is unavailable: {}", canonical.display())));
            }
            continue;
        }
        fs::read_dir(&canonical)
            .map_err(|error| Failure::Unreconciled(format!("{label} is unreadable: {}: {error}", canonical.display())))?;
        if seen.insert(canonical.clone()) {
            roots.push(Root { path: display(&canonical), provider: provider.to_owned(), account: account.to_owned(), layout });
        }
    }
    Ok(roots)
}

fn expand(path: &str, home: &Path) -> std::path::PathBuf {
    path.strip_prefix("~/").map_or_else(|| path.into(), |rest| home.join(rest))
}

fn listdir(path: &Path) -> Result<Vec<String>> {
    let unreadable = |error: std::io::Error| Failure::Unreconciled(format!("configured transcript tree is unreadable: {}: {error}", path.display()));
    let mut names = Vec::new();
    for entry in fs::read_dir(path).map_err(unreadable)? {
        names.push(entry.map_err(unreadable)?.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

struct Folder {
    dir: String,
    names: Vec<String>,
    hint: String,
}

fn claude_dirs(base: &Path) -> Result<Vec<Folder>> {
    let mut out = Vec::new();
    for project in listdir(base)? {
        let pdir = base.join(&project);
        if !pdir.is_dir() {
            continue;
        }
        let entries = listdir(&pdir)?;
        let sessions = entries.iter().filter(|entry| SESSION_FILE.is_match(entry)).cloned().collect();
        out.push(Folder { dir: display(&pdir), names: sessions, hint: project.clone() });
        for name in &entries {
            let sub = pdir.join(name).join("subagents");
            if sub.is_dir() {
                out.push(Folder { dir: display(&sub), names: listdir(&sub)?, hint: project.clone() });
            }
        }
    }
    Ok(out)
}

fn codex_dirs(dir: &Path, out: &mut Vec<Folder>) -> Result<()> {
    let names = listdir(dir)?;
    let (folders, files): (Vec<String>, Vec<String>) = names.into_iter().partition(|name| dir.join(name).is_dir());
    out.push(Folder { dir: display(dir), names: files, hint: String::new() });
    for folder in folders {
        let path = dir.join(&folder);
        if !fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            codex_dirs(&path, out)?;
        }
    }
    Ok(())
}

fn trees(root: &Root) -> Result<Vec<(String, String, String, Kind)>> {
    let tree = |provider: &str, account: &str, base: String, kind| (provider.to_owned(), account.to_owned(), base, kind);
    match root.layout {
        "claude" => Ok(vec![tree(&root.provider, &root.account, root.path.clone(), Kind::Claude)]),
        "codex" => {
            let account = if root.account.is_empty() { Path::new(&root.path).file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned()) } else { root.account.clone() };
            Ok(vec![tree(&root.provider, &account, format!("{}/sessions", root.path), Kind::Codex)])
        }
        _ => {
            let top = Path::new(&root.path);
            let mut out = Vec::new();
            for provider in listdir(top)?.into_iter().filter(|name| top.join(name).is_dir()) {
                for account in listdir(&top.join(&provider))? {
                    out.push(tree(&provider, &account, format!("{}/{provider}/{account}/sessions", root.path), Kind::Codex));
                    out.push(tree(&provider, &account, format!("{}/{provider}/{account}/projects", root.path), Kind::Claude));
                }
            }
            Ok(out)
        }
    }
}

/// Every transcript once: `logical` names the `.jsonl`, `disk` may be `<logical>.zst`, and a plain file wins over its archive.
pub(super) fn discover(roots: &[Root]) -> Result<Vec<Found>> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for root in roots {
        for (provider, account, base, kind) in trees(root)? {
            let base = Path::new(&base);
            if !base.is_dir() {
                continue;
            }
            let mut folders = Vec::new();
            if kind == Kind::Claude {
                folders = claude_dirs(base)?;
            } else {
                codex_dirs(base, &mut folders)?;
            }
            for folder in folders {
                let here: HashSet<&String> = folder.names.iter().collect();
                for name in &folder.names {
                    let (logical, disk) = if name.ends_with(".jsonl") {
                        (format!("{}/{name}", folder.dir), format!("{}/{name}", folder.dir))
                    } else if name.ends_with(".jsonl.zst") && !here.contains(&name[..name.len() - 4].to_owned()) {
                        (format!("{}/{}", folder.dir, &name[..name.len() - 4]), format!("{}/{name}", folder.dir))
                    } else {
                        continue;
                    };
                    let Ok(meta) = fs::metadata(&disk) else { continue };
                    let stat = Stat::of(&meta);
                    if seen.insert((stat.dev, stat.ino)) {
                        found.push(Found { logical, disk, provider: provider.clone(), account: account.clone(), hint: folder.hint.clone(), kind, stat });
                    }
                }
            }
        }
    }
    Ok(found)
}
