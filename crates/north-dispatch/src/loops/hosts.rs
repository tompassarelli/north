use super::{Loop, Reading, Role};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

pub type Ages = BTreeMap<String, Option<u64>>;

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

    fn place(&self, role: Role) -> Result<Option<&str>, String> {
        let host = match role {
            Role::Here => return Ok(None),
            Role::Ledger => self.ledger.as_deref(),
            Role::Lander => Some(self.lander.as_deref().map_err(String::clone)?),
        };
        Ok(host.filter(|host| *host != self.here))
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

pub fn local_ages(loops: &[Loop], state: &Path) -> Ages {
    let epoch = |at: SystemTime| at.duration_since(SystemTime::UNIX_EPOCH).map(|since| since.as_secs()).ok();
    loops.iter().filter_map(|found| Some((found.name.clone(), last_written(state, found.record.as_deref()?).and_then(epoch)))).collect()
}

pub type Ssh<'a> = &'a dyn Fn(&str) -> Result<String, String>;

pub struct Reader<'a> {
    hosts: &'a Hosts,
    state: &'a Path,
    ssh: Ssh<'a>,
    asked: RefCell<BTreeMap<String, Result<Ages, String>>>,
}

impl<'a> Reader<'a> {
    pub fn new(hosts: &'a Hosts, state: &'a Path, ssh: Ssh<'a>) -> Self {
        Self { hosts, state, ssh, asked: RefCell::default() }
    }

    pub fn read(&self, found: &Loop, record: &str) -> Reading {
        match self.hosts.place(found.host) {
            Err(why) => Reading::Unread(format!("{} host unresolved: {why}", found.host.name())),
            Ok(None) => last_written(self.state, record).map_or(Reading::Never, Reading::At),
            Ok(Some(host)) => self.remote(host, found),
        }
    }

    fn remote(&self, host: &str, found: &Loop) -> Reading {
        let mut asked = self.asked.borrow_mut();
        let ages = asked.entry(host.to_owned()).or_insert_with(|| {
            let text = (self.ssh)(host)?;
            serde_json::from_str::<Ages>(&text).map_err(|error| format!("unreadable reply: {error}"))
        });
        match ages {
            Err(why) => Reading::Unread(format!("{} {host}: {why}", found.host.name())),
            Ok(ages) => match ages.get(&found.name) {
                Some(Some(secs)) => Reading::At(SystemTime::UNIX_EPOCH + Duration::from_secs(*secs)),
                Some(None) => Reading::Never,
                None => Reading::Unread(format!("{} {host}: reported no age for {}", found.host.name(), found.name)),
            },
        }
    }
}

pub fn ssh_ages(host: &str) -> Result<String, String> {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let argv = crate::platform::ssh::argv(&["ssh".into()], &env, &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, "north", "loops", "ages", "--json"]);
    let (program, arguments) = argv.split_first().ok_or("no ssh program")?;
    let out = Command::new(program).args(arguments).output().map_err(|error| format!("ssh: {error}"))?;
    if out.status.success() { Ok(String::from_utf8_lossy(&out.stdout).into_owned()) } else { Err(format!("ssh exit {}", out.status.code().map_or_else(|| "signal".to_owned(), |code| code.to_string()))) }
}
