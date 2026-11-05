use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustix::fs::{FlockOperation, flock};

const START_LIMIT: Duration = Duration::from_secs(5);

pub struct Place {
    root: PathBuf,
}

impl Place {
    pub fn from_env() -> Self {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|runtime| !runtime.is_empty()).map(PathBuf::from);
        let runtime = runtime.unwrap_or_else(|| std::env::temp_dir().join(format!("north-{}", rustix::process::getuid().as_raw())));
        Self::at(runtime.join("north-gh-broker"))
    }

    pub fn at(root: PathBuf) -> Self {
        let _ = fs::DirBuilder::new().recursive(true).mode(0o700).create(&root);
        Self { root }
    }

    pub fn socket(&self) -> PathBuf {
        self.root.join("broker.sock")
    }

    pub fn calls(&self) -> PathBuf {
        self.root.join("calls.log")
    }

    pub fn exclusive(&self) -> Option<fs::File> {
        let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.root.join("broker.lock")).ok()?;
        flock(&file, FlockOperation::NonBlockingLockExclusive).ok()?;
        Some(file)
    }

    pub fn gh_config(&self, source: &Path) -> std::io::Result<PathBuf> {
        let directory = self.root.join("gh-config");
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&directory)?;
        let original = fs::read_to_string(source.join("config.yml")).unwrap_or_default();
        let mut config: String = original.lines().filter(|line| !line.starts_with("http_unix_socket:")).map(|line| format!("{line}\n")).collect();
        config.push_str(&format!("http_unix_socket: {}\n", self.socket().display()));
        let staged = directory.join(format!("config.yml.{}", std::process::id()));
        fs::write(&staged, config)?;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o600))?;
        fs::rename(&staged, directory.join("config.yml"))?;
        let hosts = directory.join("hosts.yml");
        let target = source.join("hosts.yml");
        if fs::read_link(&hosts).ok().as_deref() != Some(target.as_path()) {
            let staged = directory.join(format!("hosts.yml.{}", std::process::id()));
            std::os::unix::fs::symlink(&target, &staged)?;
            fs::rename(&staged, &hosts)?;
        }
        Ok(directory)
    }

    pub fn ensure_running(&self) -> Result<(), String> {
        if UnixStream::connect(self.socket()).is_ok() {
            return Ok(());
        }
        let north = std::env::current_exe().map_err(|error| format!("cannot find the north binary: {error}"))?;
        let log = fs::OpenOptions::new().create(true).append(true).open(self.root.join("broker.log")).map_err(|error| error.to_string())?;
        Command::new(north)
            .args(["gh-broker", "serve"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .process_group(0)
            .spawn()
            .map_err(|error| format!("cannot start it: {error}"))?;
        let started = Instant::now();
        while started.elapsed() < START_LIMIT {
            if UnixStream::connect(self.socket()).is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Err(format!("its socket {} did not answer within {} s; see {}", self.socket().display(), START_LIMIT.as_secs(), self.root.join("broker.log").display()))
    }
}

fn source_config() -> PathBuf {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    var("GH_CONFIG_DIR")
        .filter(|directory| !directory.ends_with("north-gh-broker/gh-config"))
        .or_else(|| var("XDG_CONFIG_HOME").map(|config| config.join("gh")))
        .unwrap_or_else(|| var("HOME").unwrap_or_default().join(".config/gh"))
}

pub fn route(command: &mut Command) -> Result<(), String> {
    let place = Place::from_env();
    place.ensure_running().map_err(|reason| format!("the north GitHub broker is not running: {reason}"))?;
    let config = place.gh_config(&source_config()).map_err(|error| format!("cannot write the broker's gh config: {error}"))?;
    command.env("GH_CONFIG_DIR", config);
    Ok(())
}
