//! The cargo config and subcommands a run outside the worker sandbox would take from places every worker sandbox may write (worker_sandbox/removable.rs and writable.rs grant ~/.cargo, ~/.cache and each project's worktrees directory), so a planted build wrapper or `cargo-SUBCOMMAND` there would run in a lander check, runtime build or eval build. Cargo reads `.cargo/config(.toml)` in every ancestor of its working directory and in its home, and looks for subcommands in its home's `bin` before PATH. Of the ancestors, the ones a worker can write and no checkout owns are the containers of land-offload slots and lanes and the directories between them and the home directory, the only place a worker writes; a checkout's own `.cargo`, and a build root's, is never one of them. Bun's global bunfig is a dotfile and rustup is on no build's PATH.
use std::path::{Path, PathBuf};

fn configs(directory: &Path) -> [PathBuf; 2] {
    [directory.join("config"), directory.join("config.toml")]
}

/// What cargo would take from outside the run's checkout here: `config` and `config.toml` in `$CARGO_HOME` (else `~/.cargo`) and in the `.cargo` of every ancestor of `directory`, at or under the home directory, that holds or is `~/.cache/land-offload` or its project's `~/code/PROJECT/worktrees`, and every `cargo-*` in `$CARGO_HOME/bin` unless the run's cargo is one a worker can already rewrite.
pub fn planted(directory: &Path) -> Vec<PathBuf> {
    let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let cargo = set("CARGO_HOME").or_else(|| set("HOME").map(|home| home.join(".cargo")));
    let home = set("HOME");
    let project = home.as_ref().and_then(|home| directory.strip_prefix(home.join("code")).ok()?.components().next().map(|project| home.join("code").join(project).join("worktrees")));
    let containers: Vec<PathBuf> = home.iter().map(|home| home.join(".cache/land-offload")).chain(project).collect();
    let above = directory.ancestors().filter(|ancestor| home.as_ref().is_some_and(|home| ancestor.starts_with(home)) && containers.iter().any(|container| container.starts_with(ancestor)));
    let mut found: Vec<PathBuf> = cargo.iter().flat_map(|cargo| configs(cargo)).chain(above.flat_map(|ancestor| configs(&ancestor.join(".cargo")))).filter(|path| path.exists()).collect();
    found.extend(cargo.iter().filter(|cargo| !rewritable_cargo(cargo, home.as_deref())).flat_map(|cargo| std::fs::read_dir(cargo.join("bin")).into_iter().flatten().flatten()).map(|entry| entry.path()).filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("cargo-"))));
    found.sort();
    found.dedup();
    found
}

/// Whether the run's cargo, the first `cargo` on PATH that is no capacity gate shim, resolves under `cargo_home` or `~/.rustup`, as a rustup install's does: a worker can rewrite that cargo itself, so a `cargo-*` beside it adds nothing to refuse.
fn rewritable_cargo(cargo_home: &Path, home: Option<&Path>) -> bool {
    let rewritable: Vec<PathBuf> = std::iter::once(cargo_home.to_owned()).chain(home.map(|home| home.join(".rustup"))).filter_map(|directory| std::fs::canonicalize(directory).ok()).collect();
    let run = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|directory| directory.join("cargo")).find(|cargo| cargo.is_file() && !super::gate_shim::is_shim(cargo));
    run.and_then(|cargo| std::fs::canonicalize(cargo).ok()).is_some_and(|real| rewritable.iter().any(|directory| real.starts_with(directory)))
}

/// Why `who` runs nothing from the current directory: a cargo config or subcommand in a place a fleet worker can write, which no build on these hosts uses.
pub fn refusal(who: &str) -> Option<String> {
    let planted = planted(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
    (!planted.is_empty()).then(|| {
        let named: Vec<String> = planted.iter().map(|path| path.display().to_string()).collect();
        format!("{who}: {} would steer this run's cargo from a place a fleet worker can write, so it runs nothing; no build here uses such a file, so remove it", named.join(", "))
    })
}
