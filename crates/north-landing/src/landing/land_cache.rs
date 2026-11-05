use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::clock::Timestamp;
use sha2::{Digest, Sha256};

pub const USAGE: &str = "usage: north land-cache key|hit|record --repo DIR --head REV --command CMD [--host-class CLASS] [--status N] [--state DIR]";
const ABSENT: &str = "absent";
const TOOLCHAIN: [&str; 3] = ["flake.lock", "rust-toolchain.toml", "rust-toolchain"];

pub struct Machine<'a> {
    pub host_class: &'a str,
    pub south: &'a Path,
    pub nixos_config: &'a Path,
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(repo).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8(output.stdout).ok()).flatten().map(|text| text.trim_end().to_owned())
}

fn filter_patterns(repo: &Path, head: &str, command: &str) -> Option<Vec<String>> {
    let blob = format!("{head}:.safe-push");
    let entries = git(repo, &["config", "--blob", &blob, "--get-all", "land.check"])?;
    let mut patterns: Vec<String> = entries
        .lines()
        .filter_map(|entry| entry.split_once(" :: "))
        .filter(|(_, entry_command)| *entry_command == command)
        .flat_map(|(paths, _)| paths.split(','))
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
        .collect();
    patterns.sort();
    patterns.dedup();
    (!patterns.is_empty()).then_some(patterns)
}

fn object_id(repo: &Path, head: &str, pattern: &str) -> String {
    let spec = if pattern.contains(['*', '?', '[']) {
        format!("{head}^{{tree}}")
    } else {
        format!("{head}:{}", pattern.trim_end_matches('/'))
    };
    git(repo, &["rev-parse", "--verify", "--quiet", &spec]).unwrap_or_else(|| ABSENT.to_owned())
}

pub fn machine_bound(command: &str) -> bool {
    ["code/south", "code/nixos-config", "machine-bound"].iter().any(|marker| command.contains(marker))
}

pub(super) fn checkout_head(root: &Path) -> String {
    git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).unwrap_or_else(|| ABSENT.to_owned())
}

pub fn key(repo: &Path, head: &str, command: &str, machine: &Machine) -> Option<String> {
    let patterns = filter_patterns(repo, head, command)?;
    let mut hasher = Sha256::new();
    hasher.update(format!("command\0{command}\n"));
    hasher.update(format!("host-class\0{}\n", machine.host_class));
    for toolchain in TOOLCHAIN {
        hasher.update(format!("{toolchain}\0{}\n", object_id(repo, head, toolchain)));
    }
    for pattern in &patterns {
        hasher.update(format!("{pattern}\0{}\n", object_id(repo, head, pattern)));
    }
    if machine_bound(command) {
        hasher.update(format!("south\0{}\n", checkout_head(machine.south)));
        hasher.update(format!("nixos-config\0{}\n", checkout_head(machine.nixos_config)));
    }
    Some(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

pub struct Pass {
    pub at: Timestamp,
    pub sha: String,
}

pub struct Store(pub PathBuf);

impl Store {
    pub fn hit(&self, key: &str) -> Option<Pass> {
        let text = fs::read_to_string(self.0.join(key)).ok()?;
        let (seconds, sha) = text.trim().split_once(' ')?;
        Some(Pass { at: Timestamp::from_unix_seconds(seconds.parse().ok()?), sha: sha.to_owned() })
    }

    pub fn record(&self, key: &str, sha: &str, status: i32, now: Timestamp) -> std::io::Result<()> {
        let path = self.0.join(key);
        if status != 0 {
            return match fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            };
        }
        fs::create_dir_all(&self.0)?;
        let pending = tempfile::NamedTempFile::new_in(&self.0)?;
        fs::write(pending.path(), format!("{} {sha}\n", now.unix_seconds()))?;
        pending.persist(&path).map(|_| ()).map_err(|error| error.error)
    }
}

pub fn reused_line(command: &str, pass: &Pass) -> String {
    format!(
        "land check: {command} reused (passed for these inputs at {}, {})",
        pass.at.local_twelve_hour(),
        pass.sha.get(..12).unwrap_or(&pass.sha)
    )
}

fn default_state() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAFE_PUSH_LAND_CACHE_DIR") {
        return dir.into();
    }
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    state.join("safe-push/land-cache")
}

fn flag(arguments: &[String], name: &str) -> Option<String> {
    arguments.iter().position(|argument| argument == name).and_then(|at| arguments.get(at + 1)).cloned()
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((verb, rest)) = arguments.split_first() else {
        eprintln!("{USAGE}");
        return 64;
    };
    let (Some(head), Some(command)) = (flag(rest, "--head"), flag(rest, "--command")) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let repo = PathBuf::from(flag(rest, "--repo").unwrap_or_else(|| ".".to_owned()));
    let host_class = flag(rest, "--host-class").unwrap_or_else(|| "local".to_owned());
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let (south, nixos_config) = (home.join("code/south/main"), home.join("code/nixos-config/main"));
    let Some(key) = key(&repo, &head, &command, &Machine { host_class: &host_class, south: &south, nixos_config: &nixos_config }) else {
        eprintln!("north land-cache: no land.check entry runs {command:?} at {head}");
        return 2;
    };
    let store = Store(flag(rest, "--state").map(PathBuf::from).unwrap_or_else(default_state));
    match verb.as_str() {
        "key" => {
            println!("{key}");
            0
        }
        "hit" => match store.hit(&key) {
            Some(pass) => {
                println!("{}", reused_line(&command, &pass));
                0
            }
            None => 1,
        },
        "record" => {
            let status = flag(rest, "--status").and_then(|text| text.parse().ok()).unwrap_or(0);
            let sha = git(&repo, &["rev-parse", "--verify", &format!("{head}^{{commit}}")]).unwrap_or_else(|| head.clone());
            match store.record(&key, &sha, status, Timestamp::now()) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("north land-cache: {error}");
                    2
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            64
        }
    }
}

#[cfg(test)]
#[path = "land_cache_tests.rs"]
mod tests;
