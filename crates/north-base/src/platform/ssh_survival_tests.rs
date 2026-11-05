use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::argv;

fn which(program: &str) -> String {
    let found = Command::new("sh").args(["-c", &format!("command -v {program}")]).output().unwrap();
    String::from_utf8(found.stdout).unwrap().trim().to_owned()
}

const MARKER: &str = "NORTH_SSH_TEST_UNIT";

struct Fixture {
    dir: tempfile::TempDir,
    runtime: tempfile::TempDir,
    sshd: std::process::Child,
    units: Vec<(String, std::process::Child)>,
}

impl Fixture {
    fn new() -> Self {
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
            "Host testhost\n  HostName 127.0.0.1\n  Port {port}\n  User {user}\n  IdentityFile {key}\n  IdentitiesOnly yes\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n  LogLevel ERROR\n",
            key = path.join("client").display(),
        );
        std::fs::write(path.join("ssh_config"), client_config).unwrap();
        let sshd = Command::new(which("sshd")).args(["-D", "-f"]).arg(path.join("sshd_config")).stderr(Stdio::null()).spawn().unwrap();
        let fixture = Self { dir, runtime, sshd, units: Vec::new() };
        let probe = Instant::now();
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(probe.elapsed() < Duration::from_secs(10), "sshd did not listen");
            std::thread::sleep(Duration::from_millis(50));
        }
        fixture
    }

    fn env(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("XDG_RUNTIME_DIR".to_owned(), self.runtime.path().to_string_lossy().into_owned()),
            ("NORTH_SSH_CONFIG".to_owned(), self.dir.path().join("ssh_config").to_string_lossy().into_owned()),
        ])
    }

    fn client(&self, arguments: &[&str]) -> Vec<String> {
        argv(&[which("ssh").into()], &self.env(), arguments).into_iter().map(|word| word.to_string_lossy().into_owned()).collect()
    }

    fn socket_up(&self) -> bool {
        std::fs::read_dir(self.runtime.path()).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().starts_with("north-ssh-"))
    }

    fn unit(&mut self, name: &str, words: &[String]) -> String {
        let marker = format!("{name}-{}", std::process::id());
        let child = Command::new(&words[0]).args(&words[1..]).env(MARKER, &marker).stdout(Stdio::null()).spawn().unwrap();
        self.units.push((marker.clone(), child));
        marker
    }

    fn wait_for_socket(&self) {
        let probe = Instant::now();
        while !self.socket_up() {
            assert!(probe.elapsed() < Duration::from_secs(15), "no ssh master socket appeared");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn stop(&mut self, marker: &str) {
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

fn master_words(fixture: &Fixture) -> Vec<String> {
    let control = format!("ControlPath={}/north-ssh-%C", fixture.runtime.path().display());
    let mut words = vec![which("ssh"), "-F".to_owned()];
    words.push(fixture.dir.path().join("ssh_config").to_string_lossy().into_owned());
    for option in ["ControlMaster=yes", "ControlPersist=no", "BatchMode=yes"] {
        words.extend(["-o".to_owned(), option.to_owned()]);
    }
    words.extend(["-o".to_owned(), control, "-N".to_owned(), "testhost".to_owned()]);
    words
}

fn caller_status(fixture: &Fixture, seconds: &str) -> std::process::Child {
    let words = fixture.client(&["-o", "BatchMode=yes", "testhost", "sleep", seconds]);
    Command::new(&words[0]).args(&words[1..]).stdout(Stdio::null()).spawn().unwrap()
}

#[test]
fn a_multiplexed_session_survives_restarting_the_unit_that_made_the_first_connection() {
    let mut fixture = Fixture::new();
    let master = master_words(&fixture);
    fixture.unit("north-ssh-master-test", &master);
    fixture.wait_for_socket();

    let first = fixture.client(&["-o", "BatchMode=yes", "testhost", "sleep", "60"]);
    let first_unit = fixture.unit("north-ssh-first-test", &first);
    let mut second = caller_status(&fixture, "4");
    std::thread::sleep(Duration::from_millis(1500));
    fixture.stop(&first_unit);

    let status = second.wait().unwrap();
    assert_eq!(status.code(), Some(0), "the multiplexed session must outlive the first caller's unit");
}

