//! The cargo config and subcommands a run outside the worker sandbox would take from places every worker sandbox may write (worker_sandbox/removable.rs and writable.rs grant ~/.cargo, ~/.cache and each project's worktrees directory), so a planted build wrapper or `cargo-SUBCOMMAND` there would run in a lander check, runtime build or eval build. Cargo reads `.cargo/config(.toml)` in every ancestor of its working directory and in its home, and looks for subcommands in its home's `bin` before PATH. Of the ancestors, the ones a worker can write and no checkout owns are the containers of land-offload slots and lanes and the directories above them; a checkout's own `.cargo`, and a build root's, is never one of them. Bun's global bunfig is a dotfile and rustup is on no build's PATH.
use std::path::{Path, PathBuf};

fn configs(directory: &Path) -> [PathBuf; 2] {
    [directory.join("config"), directory.join("config.toml")]
}

/// What cargo would take from outside the run's checkout here: `config` and `config.toml` in `$CARGO_HOME` (else `~/.cargo`) and in the `.cargo` of every ancestor of `directory` that holds or is `~/.cache/land-offload` or its project's `~/code/PROJECT/worktrees`, and every `cargo-*` in `$CARGO_HOME/bin`.
pub fn planted(directory: &Path) -> Vec<PathBuf> {
    let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let cargo = set("CARGO_HOME").or_else(|| set("HOME").map(|home| home.join(".cargo")));
    let home = set("HOME");
    let project = home.as_ref().and_then(|home| directory.strip_prefix(home.join("code")).ok()?.components().next().map(|project| home.join("code").join(project).join("worktrees")));
    let containers: Vec<PathBuf> = home.iter().map(|home| home.join(".cache/land-offload")).chain(project).collect();
    let above = directory.ancestors().filter(|ancestor| containers.iter().any(|container| container.starts_with(ancestor)));
    let mut found: Vec<PathBuf> = cargo.iter().flat_map(|cargo| configs(cargo)).chain(above.flat_map(|ancestor| configs(&ancestor.join(".cargo")))).filter(|path| path.exists()).collect();
    found.extend(cargo.iter().flat_map(|cargo| std::fs::read_dir(cargo.join("bin")).into_iter().flatten().flatten()).map(|entry| entry.path()).filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("cargo-"))));
    found.sort();
    found.dedup();
    found
}

/// Why `who` runs nothing from the current directory: a cargo config or subcommand in a place a fleet worker can write, which no build on these hosts uses.
pub fn refusal(who: &str) -> Option<String> {
    let planted = planted(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
    (!planted.is_empty()).then(|| {
        let named: Vec<String> = planted.iter().map(|path| path.display().to_string()).collect();
        format!("{who}: {} would steer this run's cargo from a place a fleet worker can write, so it runs nothing; no build here uses such a file, so remove it", named.join(", "))
    })
}
