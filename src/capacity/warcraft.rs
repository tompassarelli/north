use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const PREFIX_VARIABLES: [&str; 2] = ["STEAM_COMPAT_DATA_PATH", "WINEPREFIX"];

pub(crate) fn run() -> u8 {
    let root = std::env::var_os("AGENT_CAPACITY_PROC").unwrap_or_else(|| "/proc".into());
    println!("{}", clients(Path::new(&root)));
    0
}

/// Live Warcraft III clients: one per Wine prefix, so a client running two processes counts once.
fn clients(proc_root: &Path) -> usize {
    let Ok(entries) = fs::read_dir(proc_root) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()))
        .filter(|entry| fs::read_to_string(entry.path().join("comm")).is_ok_and(|comm| comm.starts_with("Warcraft III")))
        .map(|entry| prefix(&entry.path()).unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned()))
        .collect::<BTreeSet<_>>()
        .len()
}

fn prefix(process: &Path) -> Option<String> {
    let environment = fs::read(process.join("environ")).ok()?;
    PREFIX_VARIABLES.iter().find_map(|name| {
        environment
            .split(|byte| *byte == 0)
            .find_map(|entry| entry.strip_prefix(format!("{name}=").as_bytes()))
            .filter(|value| !value.is_empty())
            .map(|value| format!("prefix:{}", String::from_utf8_lossy(value)))
    })
}
