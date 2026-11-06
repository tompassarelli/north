use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::facts::{Place, Reach};

pub const UNIT_PREFIX: &str = "north-drill-";

/// A scratch lander under `root`: its own home, state, runtime directory, git config and `north` stand-in, and the processes and user
/// units a fault started, which it ends when dropped. Nothing in it names the real lander's queues, links, slots or units.
pub struct Lander {
    pub root: PathBuf,
    pub id: String,
    jobs: Vec<Child>,
    units: Vec<String>,
}

impl Lander {
    pub fn open(root: &Path) -> Result<Self, String> {
        let named = root.components().rev().take(2).map(|part| part.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("");
        let id = named.replace("north-drill-", "").replace(|letter: char| !letter.is_ascii_alphanumeric(), "");
        let lander = Self { root: root.to_owned(), id, jobs: Vec::new(), units: Vec::new() };
        for directory in [lander.home(), lander.state(), lander.run(), lander.bin()] {
            fs::create_dir_all(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
        }
        write(&lander.gitconfig(), "[user]\n\tname = north drill\n\temail = drill@north.invalid\n[commit]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n")?;
        let calls = lander.root.join("north-calls.log");
        script(&lander.north(), &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n", calls.display()))?;
        Ok(lander)
    }

    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    pub fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    fn run(&self) -> PathBuf {
        self.root.join("run")
    }

    fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    pub fn gitconfig(&self) -> PathBuf {
        self.root.join("gitconfig")
    }

    /// A `north` that only records its arguments, for the incident rows and alarms a remedy's own program would otherwise write.
    pub fn north(&self) -> PathBuf {
        self.bin().join("north")
    }

    pub fn unit(&self, fault: &str) -> String {
        format!("{UNIT_PREFIX}{}-{fault}.service", self.id)
    }

    /// The environment every scratch command runs under; `north` on its PATH is the binary running the drill.
    pub fn env(&self) -> Vec<(String, String)> {
        let own = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)).unwrap_or_default();
        let path = format!("{}:{}", own.display(), std::env::var("PATH").unwrap_or_default());
        [
            ("HOME", self.home()),
            ("XDG_STATE_HOME", self.state()),
            ("XDG_RUNTIME_DIR", self.run()),
            ("AGENT_CAPACITY_NORTH", self.north()),
            ("NORTH_LAND_NORTH", self.north()),
            ("GIT_CONFIG_GLOBAL", self.gitconfig()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.display().to_string()))
        .chain([("GIT_CONFIG_NOSYSTEM".to_owned(), "1".to_owned()), ("PATH".to_owned(), path)])
        .collect()
    }

    pub fn place(&self, namespace: Option<u32>) -> Place {
        Place { name: format!("scratch-{}", self.id), reach: Reach::Scratch { env: self.env(), namespace } }
    }

    pub fn git(&self, directory: &Path, arguments: &[&str]) -> Result<String, String> {
        let output = Command::new("git").arg("-C").arg(directory).args(arguments).envs(self.env()).stdin(Stdio::null()).output().map_err(|error| format!("git: {error}"))?;
        if !output.status.success() {
            return Err(format!("git {} in {}: {}", arguments.join(" "), directory.display(), String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Starts `script` (bash, `$1` = `argument`, under `program` when one is named) in its own process group under the scratch environment and returns its pid once it
    /// prints its first line, which it must print only when the state it holds is in place.
    pub fn hold(&mut self, program: &[&str], script: &str, argument: &Path) -> Result<u32, String> {
        let command: Vec<&str> = program.iter().copied().chain(["bash", "-c", script, "drill"]).collect();
        let mut child = Command::new(command[0])
            .args(&command[1..])
            .arg(argument)
            .envs(self.env())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|error| format!("{}: {error}", command[0]))?;
        let mut said = String::new();
        let read = child.stdout.take().map(|stdout| BufReader::new(stdout).read_line(&mut said));
        let pid = child.id();
        self.jobs.push(child);
        match read {
            Some(Ok(count)) if count > 0 => Ok(pid),
            _ => Err(format!("{} ended before it held its state", command.join(" "))),
        }
    }

    /// Starts a scratch user unit running `script` (bash, `$1` = `argument`), which this lander stops when dropped.
    pub fn start_unit(&mut self, unit: &str, script: &str, argument: &Path) -> Result<(), String> {
        let output = Command::new("systemd-run")
            .args(["--user", "--quiet", "--collect", &format!("--unit={unit}"), "bash", "-c", script, "drill"])
            .arg(argument)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("systemd-run: {error}"))?;
        if !output.status.success() {
            return Err(format!("systemd-run --unit={unit}: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        self.units.push(unit.to_owned());
        Ok(())
    }
}

impl Drop for Lander {
    fn drop(&mut self) {
        for unit in self.units.iter().filter(|unit| unit.starts_with(UNIT_PREFIX)) {
            let _ = Command::new("systemctl").args(["--user", "stop", unit]).stdin(Stdio::null()).stderr(Stdio::null()).status();
        }
        for child in self.jobs.iter_mut() {
            let _ = Command::new("kill").args(["-KILL", &format!("-{}", child.id())]).stderr(Stdio::null()).status();
            let _ = child.wait();
        }
    }
}

pub fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn script(path: &Path, text: &str) -> Result<(), String> {
    write(path, "")?;
    north_base::foundation::shim::write_executable(path, text);
    Ok(())
}

/// Sets `path`'s modification time `seconds` into the past, which is how a fault ages a file past the doctor's bound.
pub fn age(path: &Path, seconds: u64) -> Result<(), String> {
    let then = SystemTime::now() - Duration::from_secs(seconds);
    fs::File::options().write(true).open(path).and_then(|file| file.set_modified(then)).map_err(|error| format!("{}: {error}", path.display()))
}
