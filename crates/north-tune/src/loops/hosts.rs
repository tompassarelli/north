use super::Role;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

pub struct Hosts {
    pub here: String,
    pub ledger: Option<String>,
    pub lander: Result<String, String>,
}

impl Hosts {
    pub fn read(home: &Path, harness: Result<PathBuf, String>) -> Self {
        let name = |text: String| text.trim().rsplit('@').next().unwrap_or_default().to_owned();
        Self {
            here: crate::platform::host::name(),
            ledger: std::fs::read_to_string(home.join(".config/threads/remote")).ok().map(name).filter(|host| !host.is_empty()),
            lander: harness.and_then(|root| lander_host(&root)),
        }
    }

    pub fn holds(&self, role: Role) -> bool {
        match role {
            Role::Here => true,
            Role::Ledger => self.ledger.as_deref().is_none_or(|host| host == self.here),
            Role::Lander => self.lander.as_deref().is_ok_and(|host| host == self.here),
        }
    }
}

fn lander_host(root: &Path) -> Result<String, String> {
    let out = Command::new("git").arg("config").arg("-f").arg(root.join(".safe-push")).args(["--get", "landing.lander-host"]).output().map_err(|error| format!("git config: {error}"))?;
    let host = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if out.status.success() && !host.is_empty() { Ok(host) } else { Err(format!("{} names no landing.lander-host", root.join(".safe-push").display())) }
}

pub fn last_written(root: &Path, record: &str) -> Option<SystemTime> {
    let path = root.join(record);
    let own = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok()?;
    let entries = std::fs::read_dir(&path).into_iter().flatten().flatten().filter_map(|entry| entry.metadata().and_then(|meta| meta.modified()).ok());
    entries.chain([own]).max()
}
