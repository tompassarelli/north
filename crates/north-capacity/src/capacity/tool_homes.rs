//! The cargo config a run outside the worker sandbox would read from cargo's home, which every worker sandbox may write (worker_sandbox/removable.rs grants ~/.cargo, ~/.rustup and ~/.bun), so a planted build wrapper there would run in a lander check, runtime build or eval build. Of those homes only cargo's holds a config file the tool reads: bun's global bunfig is a dotfile and rustup is on no build's PATH.
use std::path::PathBuf;

/// The config files cargo would read from its home here: `config` and `config.toml` in `$CARGO_HOME`, else `~/.cargo`.
pub fn planted() -> Vec<PathBuf> {
    let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let cargo = set("CARGO_HOME").or_else(|| set("HOME").map(|home| home.join(".cargo")));
    cargo.iter().flat_map(|cargo| [cargo.join("config"), cargo.join("config.toml")]).filter(|path| path.exists()).collect()
}

/// Why `who` runs nothing here: a config file in cargo's worker-writable home, which no build on these hosts uses.
pub fn refusal(who: &str) -> Option<String> {
    let planted = planted();
    (!planted.is_empty()).then(|| {
        let named: Vec<String> = planted.iter().map(|path| path.display().to_string()).collect();
        format!("{who}: {} would steer this run's cargo from a home a fleet worker can write, so it runs nothing; no build here reads such a file, so remove it", named.join(", "))
    })
}
