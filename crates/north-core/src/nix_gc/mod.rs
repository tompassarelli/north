//! `north nix gc`: root what the hosts still run, then collect the store.
mod rustup;
mod shells;

pub use rustup::{Reference as RustupReference, missing as missing_rustup, references as rustup_references};
pub use shells::{Shell, live as live_shells};
pub use north_base::platform::store_refs::{RUNTIME_PROGRAMS, paths as store_paths, root_of as store_root, roots as store_roots};

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: north nix gc [--dry-run] [--max-freed SIZE]";
const OFFLINE: [&str; 2] = ["--offline", "--no-update-lock-file"];

pub struct Env {
    pub code: PathBuf,
    pub state: PathBuf,
    pub rustup_home: PathBuf,
}

impl Env {
    pub fn from_env() -> Self {
        let get = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let home = get("HOME").unwrap_or_default();
        let state = get("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state"));
        Self { code: home.join("code"), state: state.join("north"), rustup_home: get("RUSTUP_HOME").unwrap_or_else(|| home.join(".rustup")) }
    }

    pub fn dev_shell_roots(&self) -> PathBuf {
        self.state.join("dev-shells")
    }

    pub fn dev_shell_root(&self, shell: &Shell) -> PathBuf {
        self.dev_shell_roots().join(&shell.name)
    }

    fn runtimes(&self) -> PathBuf {
        self.state.join("runtimes")
    }

    fn rustup_roots(&self) -> PathBuf {
        self.state.join("rustup-roots")
    }
}

pub trait Host {
    fn run(&self, program: &str, args: &[String]) -> Result<String, String>;
}

pub struct System;

impl Host for System {
    fn run(&self, program: &str, args: &[String]) -> Result<String, String> {
        let output = Command::new(program).args(args).output().map_err(|error| format!("{program} did not run: {error}"))?;
        match output.status.success() {
            true => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
            false => Err(String::from_utf8_lossy(&output.stderr).lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")),
        }
    }
}

pub fn offline_args(leading: &[&str], trailing: &[&str]) -> Vec<String> {
    leading.iter().chain(OFFLINE.iter()).chain(trailing.iter()).map(|word| (*word).to_owned()).collect()
}

#[derive(Debug, Eq, PartialEq)]
pub enum Kind {
    Runtime,
    DevShell,
    Rustup,
}

impl Kind {
    fn label(&self) -> &'static str {
        match self {
            Kind::Runtime => "runtime",
            Kind::DevShell => "dev-shell",
            Kind::Rustup => "rustup",
        }
    }
}

pub struct Root {
    pub kind: Kind,
    pub owner: String,
    pub source: String,
    pub store: Option<String>,
    pub link: PathBuf,
}

impl Root {
    fn present(&self) -> bool {
        Path::new(self.store.as_deref().unwrap_or(&self.source)).exists()
    }
}

pub fn plan(env: &Env) -> Vec<Root> {
    let mut roots = Vec::new();
    let mut runtimes: Vec<PathBuf> = fs::read_dir(env.runtimes()).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
    runtimes.sort();
    for runtime in runtimes {
        let owner: String = runtime.file_name().map(|name| name.to_string_lossy().chars().take(8).collect()).unwrap_or_default();
        let wanted: BTreeSet<String> = RUNTIME_PROGRAMS.iter().flat_map(|program| store_roots(&fs::read(runtime.join(program)).unwrap_or_default())).collect();
        for store in wanted {
            let link = runtime.join(format!("gcroot-{}", store.rsplit('/').next().unwrap_or_default()));
            roots.push(Root { kind: Kind::Runtime, owner: owner.clone(), source: store.clone(), store: Some(store), link });
        }
    }
    for shell in live_shells(&env.code) {
        let owner = match shell.also {
            0 => shell.name.clone(),
            more => format!("{} (+{more} worktrees)", shell.name),
        };
        roots.push(Root { kind: Kind::DevShell, owner, source: shell.worktree.display().to_string(), store: None, link: env.dev_shell_root(&shell) });
    }
    let mut seen = BTreeSet::new();
    for reference in rustup_references(&env.rustup_home) {
        if seen.insert(reference.path.clone()) {
            let store = store_root(&reference.path);
            let link = env.rustup_roots().join(store.rsplit('/').next().unwrap_or_default());
            roots.push(Root { kind: Kind::Rustup, owner: reference.toolchain, source: reference.path, store: Some(store), link });
        }
    }
    roots
}

pub fn render(roots: &[Root]) -> String {
    roots
        .iter()
        .map(|root| {
            let state = match (&root.kind, root.present()) {
                (_, false) => " (missing from the store)",
                (Kind::DevShell, true) if root.link.exists() => " (rooted)",
                (Kind::DevShell, true) => " (to root)",
                _ => "",
            };
            format!("{:<9} {} {}{state}", root.kind.label(), root.owner, root.source)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn root_one(root: &Root, host: &dyn Host) -> Result<(), String> {
    if let Some(parent) = root.link.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let link = root.link.display().to_string();
    match &root.store {
        Some(store) if root.present() => host.run("nix-store", &["--add-root".into(), link, "--indirect".into(), "--realise".into(), store.clone()]).map(drop),
        Some(_) => Ok(()),
        None if root.link.exists() => Ok(()),
        None => host.run("nix", &offline_args(&["print-dev-env", &root.source], &["--profile", &link])).map(drop),
    }
}

fn prune(env: &Env, roots: &[Root]) {
    let keep: BTreeSet<String> = roots.iter().filter(|root| root.kind == Kind::DevShell).filter_map(|root| root.link.file_name().map(|name| name.to_string_lossy().into_owned())).collect();
    for entry in fs::read_dir(env.dev_shell_roots()).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let base = match name.strip_suffix("-link") {
            Some(stem) => stem.rsplit_once('-').map_or(name.as_str(), |(base, _)| base),
            None => name.as_str(),
        };
        if !keep.contains(base) && entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub fn run(arguments: &[String], env: &Env, host: &dyn Host) -> u8 {
    let mut dry = false;
    let mut max_freed = None;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--dry-run" => dry = true,
            "--max-freed" => match words.next() {
                Some(size) => max_freed = Some(size.clone()),
                None => return usage(),
            },
            other => match other.strip_prefix("--max-freed=") {
                Some(size) => max_freed = Some(size.to_owned()),
                None => return usage(),
            },
        }
    }
    let roots = plan(env);
    println!("{}", render(&roots));
    if dry {
        println!("north nix gc: dry run, {} roots listed, nothing rooted or collected", roots.len());
        return 0;
    }
    for root in &roots {
        if let Err(error) = root_one(root, host) {
            eprintln!("north nix gc: could not root {} {}: {error}\nnorth nix gc: nothing collected", root.kind.label(), root.source);
            return 1;
        }
    }
    prune(env, &roots);
    let mut args = Vec::new();
    args.extend(max_freed.into_iter().flat_map(|size| ["--max-freed".to_owned(), size]));
    match host.run("nix-collect-garbage", &args) {
        Ok(out) => {
            print!("{out}");
            0
        }
        Err(error) => {
            eprintln!("north nix gc: nix-collect-garbage failed: {error}");
            1
        }
    }
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

#[cfg(test)]
pub(crate) mod tests;
