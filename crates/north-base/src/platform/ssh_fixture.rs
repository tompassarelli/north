use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::ssh::argv;
use super::wait;

pub fn which(program: &str) -> String {
    let found = Command::new("sh").args(["-c", &format!("command -v {program}")]).output().unwrap();
    String::from_utf8(found.stdout).unwrap().trim().to_owned()
}

const MARKER: &str = "NORTH_SSH_TEST_UNIT";

// Cranelift test builds run no destructors when a test panics, so each daemon dies with its test thread, not in Fixture::drop.
fn daemon(program: &str) -> Command {
    let mut command = Command::new("setpriv");
    command.args(["--pdeathsig", "KILL", "--", program]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command
}

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub runtime: tempfile::TempDir,
    sshd: std::process::Child,
    units: Vec<(String, std::process::Child)>,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let runtime = tempfile::Builder::new().prefix("ns").tempdir_in("/tmp").unwrap();
        let path = dir.path();
        for key in ["host", "client"] {
            let status = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-f"]).arg(path.join(key)).status().unwrap();
            assert!(status.success());
        }
        std::fs::copy(path.join("client.pub"), path.join("authorized_keys")).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let sshd_config = format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {host}\nAuthorizedKeysFile {keys}\nPidFile {pid}\nUsePAM no\nStrictModes no\nPasswordAuthentication no\nPubkeyAuthentication yes\n",
            host = path.join("host").display(), keys = path.join("authorized_keys").display(), pid = path.join("sshd.pid").display(),
        );
        std::fs::write(path.join("sshd_config"), sshd_config).unwrap();
        let user = std::env::var("USER").unwrap_or_else(|_| "tom".to_owned());
        let client_config = format!(
            "Host testhost\n  HostName 127.0.0.1\n  Port {port}\n  User {user}\n  IdentityFile {key}\n  IdentitiesOnly yes\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n  LogLevel ERROR\n  ControlMaster no\n  ControlPath {control}/north-ssh-%C\n",
            key = path.join("client").display(), control = runtime.path().display(),
        );
        std::fs::write(path.join("ssh_config"), client_config).unwrap();
        let sshd = daemon(&which("sshd")).args(["-D", "-f"]).arg(path.join("sshd_config")).spawn().unwrap();
        let fixture = Self { dir, runtime, sshd, units: Vec::new() };
        assert!(wait::until(Duration::from_secs(10), || std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()), "sshd did not listen");
        fixture
    }

    pub fn env(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("XDG_RUNTIME_DIR".to_owned(), self.runtime.path().to_string_lossy().into_owned()),
            ("NORTH_SSH_CONFIG".to_owned(), self.dir.path().join("ssh_config").to_string_lossy().into_owned()),
        ])
    }

    pub fn client(&self, arguments: &[&str]) -> Vec<String> {
        argv(&[which("ssh").into()], &self.env(), arguments).into_iter().map(|word| word.to_string_lossy().into_owned()).collect()
    }

    fn socket_up(&self) -> bool {
        let words = self.client(&["-o", "BatchMode=yes", "-O", "check", "testhost"]);
        Command::new(&words[0]).args(&words[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
    }

    pub fn unit(&mut self, name: &str, words: &[String]) -> String {
        let marker = format!("{name}-{}", std::process::id());
        let child = daemon(&words[0]).args(&words[1..]).env(MARKER, &marker).spawn().unwrap();
        self.units.push((marker.clone(), child));
        marker
    }

    pub fn pid(&self, marker: &str) -> u32 {
        self.units.iter().find(|(name, _)| name == marker).map(|(_, child)| child.id()).unwrap()
    }

    pub fn wait_for_socket(&self) {
        assert!(wait::until(Duration::from_secs(15), || self.socket_up()), "no ssh master socket appeared");
    }

    pub fn stop(&mut self, marker: &str) {
        let wanted = format!("{MARKER}={marker}");
        for entry in std::fs::read_dir("/proc").unwrap().flatten() {
            let Ok(environ) = std::fs::read(entry.path().join("environ")) else { continue };
            if environ.split(|byte| *byte == 0).any(|pair| pair == wanted.as_bytes()) {
                let _ = Command::new("kill").args(["-KILL", &entry.file_name().to_string_lossy()]).status();
            }
        }
        for (name, child) in &mut self.units {
            if name == marker {
                let _ = child.wait();
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let markers: Vec<String> = self.units.iter().map(|(marker, _)| marker.clone()).collect();
        for marker in markers {
            self.stop(&marker);
        }
        let _ = self.sshd.kill();
        let _ = self.sshd.wait();
    }
}

pub fn master_words(fixture: &Fixture) -> Vec<String> {
    let control = format!("ControlPath={}/north-ssh-%C", fixture.runtime.path().display());
    let mut words = vec![which("ssh"), "-F".to_owned()];
    words.push(fixture.dir.path().join("ssh_config").to_string_lossy().into_owned());
    for option in ["ControlMaster=yes", "ControlPersist=no", "BatchMode=yes"] {
        words.extend(["-o".to_owned(), option.to_owned()]);
    }
    words.extend(["-o".to_owned(), control, "-N".to_owned(), "testhost".to_owned()]);
    words
}


#[path = "ssh_fixture_tests.rs"]
mod tests;
