use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

const USAGE: &str = "usage: north denylist hold SOUTH_DIR";
const IDLE: Duration = Duration::from_secs(600);
const WARM: Duration = Duration::from_secs(30);
const WATCH: Duration = Duration::from_secs(2);
const ANSWER_WITHIN: Duration = Duration::from_secs(1);
const ASK_WITHIN: Duration = Duration::from_millis(900);
const OPEN_WITHIN: Duration = Duration::from_secs(10);
const SOURCE_REF: &str = "refs/heads/main";
const EQUAL: &str = "equal";
const MOVED: &str = "moved";

#[derive(Debug, Eq, PartialEq)]
pub enum Answer {
    Equal,
    Moved,
    Absent,
    Silent,
}

pub fn dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()).map(|dir| PathBuf::from(dir).join("north-denylist"))
}

pub fn held(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| !rest.split('/').next().unwrap_or_default().contains('@'))
}

fn socket(dir: &Path, url: &str) -> PathBuf {
    let digest = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(url.as_bytes()));
    let name: String = digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect();
    dir.join(format!("{name}.sock"))
}

pub fn ask(dir: &Path, url: &str, oid: &str) -> Answer {
    let Ok(mut stream) = UnixStream::connect(socket(dir, url)) else { return Answer::Absent };
    if stream.set_read_timeout(Some(ANSWER_WITHIN)).and_then(|()| stream.set_write_timeout(Some(ANSWER_WITHIN))).is_err() || writeln!(stream, "{oid}").is_err() {
        return Answer::Silent;
    }
    let mut answer = String::new();
    let _ = (&stream).take(16).read_to_string(&mut answer);
    match answer.trim_end() {
        EQUAL => Answer::Equal,
        MOVED => Answer::Moved,
        _ => Answer::Silent,
    }
}

pub fn start(south: &Path) {
    let Ok(program) = std::env::current_exe() else { return };
    if program.file_name().is_none_or(|name| name != "north") {
        return;
    }
    let _ = Command::new(program).arg("denylist").arg("hold").arg(south).process_group(0).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

pub fn hold(arguments: &[String], url_of: impl Fn(&Path) -> Result<String, String>) -> u8 {
    let [south] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    let (Some(dir), Ok(url)) = (dir(), url_of(&Path::new(south).join("main"))) else { return 1 };
    if !held(&url) {
        return 1;
    }
    let mut source = Source { url: url.clone(), open: None };
    match serve(&dir, &url, move || source.advertised(), current_runtime) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("north denylist hold: {error}");
            1
        }
    }
}

pub fn serve(dir: &Path, url: &str, mut advertised: impl FnMut() -> Option<String>, runtime: fn() -> Option<PathBuf>) -> io::Result<()> {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let socket = socket(dir, url);
    let lock = OpenOptions::new().create(true).truncate(false).write(true).mode(0o600).open(socket.with_extension("lock"))?;
    let owner = lock.metadata()?.uid();
    owner_only(dir, owner)?;
    if lock.try_lock().is_err() {
        return Ok(());
    }
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    owner_only(&socket, owner)?;
    let (arrived, calls) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if arrived.send(stream).is_err() {
                return;
            }
        }
    });
    let (started_on, mut called, mut asked) = (runtime(), Instant::now(), None::<Instant>);
    loop {
        match calls.recv_timeout(WATCH) {
            Ok(stream) => {
                called = Instant::now();
                answer(&stream, &mut advertised);
                asked = Some(Instant::now());
            }
            Err(RecvTimeoutError::Timeout) if called.elapsed() < IDLE && runtime() == started_on => {
                if asked.is_none_or(|at| at.elapsed() >= WARM) {
                    let _ = advertised();
                    asked = Some(Instant::now());
                }
            }
            Err(_) => {
                let _ = fs::remove_file(&socket);
                return Ok(());
            }
        }
    }
}

fn owner_only(path: &Path, owner: u32) -> io::Result<()> {
    let found = fs::symlink_metadata(path)?;
    let mode = found.permissions().mode() & 0o777;
    if mode & 0o077 == 0 && found.uid() == owner {
        return Ok(());
    }
    Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{} has mode {mode:o} or another owner, so it is not this user's alone", path.display())))
}

fn answer(stream: &UnixStream, advertised: &mut impl FnMut() -> Option<String>) {
    let mut line = String::new();
    if stream.set_read_timeout(Some(ANSWER_WITHIN)).is_err() || BufReader::new(stream.take(80)).read_line(&mut line).is_err() {
        return;
    }
    let oid = line.trim_end_matches('\n');
    if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return;
    }
    let Some(source) = advertised() else { return };
    let mut stream = stream;
    let _ = writeln!(stream, "{}", if source == oid { EQUAL } else { MOVED });
}

struct Source {
    url: String,
    open: Option<Helper>,
}

impl Source {
    fn advertised(&mut self) -> Option<String> {
        if self.open.is_none() {
            self.open = Helper::open(&self.url);
        }
        let found = self.open.as_mut()?.advertised();
        if found.is_none()
            && let Some(mut helper) = self.open.take()
        {
            let _ = helper.child.kill();
            let _ = helper.child.wait();
        }
        found
    }
}

struct Helper {
    child: Child,
    input: ChildStdin,
    replies: mpsc::Receiver<String>,
}

impl Helper {
    fn open(url: &str) -> Option<Self> {
        let mut child = Command::new("git")
            .args(["remote-https", "origin", url])
            .env("GIT_PROTOCOL", "version=2")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let (Some(mut input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        let (sent, replies) = mpsc::channel();
        std::thread::spawn(move || replies_of(output, &sent));
        let opened = input.write_all(b"stateless-connect git-upload-pack\n").is_ok() && replies.recv_timeout(OPEN_WITHIN).is_ok();
        if !opened {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        Some(Self { child, input, replies })
    }

    fn advertised(&mut self) -> Option<String> {
        let request = [packet("command=ls-refs\n"), "0001".to_owned(), packet(&format!("ref-prefix {SOURCE_REF}\n")), "0000".to_owned()].concat();
        self.input.write_all(request.as_bytes()).and_then(|()| self.input.flush()).ok()?;
        self.replies.recv_timeout(ASK_WITHIN).ok().filter(|oid| !oid.is_empty())
    }
}

fn packet(line: &str) -> String {
    format!("{:04x}{line}", line.len() + 4)
}

enum Packet {
    Flush,
    Delim,
    End,
    Data(Vec<u8>),
}

fn read_packet(reader: &mut impl Read) -> Option<Packet> {
    let mut length = [0; 4];
    reader.read_exact(&mut length).ok()?;
    match usize::from_str_radix(std::str::from_utf8(&length).ok()?, 16).ok()? {
        0 => Some(Packet::Flush),
        1 => Some(Packet::Delim),
        2 => Some(Packet::End),
        3 => None,
        length => {
            let mut data = vec![0; length - 4];
            reader.read_exact(&mut data).ok()?;
            Some(Packet::Data(data))
        }
    }
}

fn replies_of(output: ChildStdout, sent: &mpsc::Sender<String>) {
    let mut reader = BufReader::new(output);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || !line.trim().is_empty() {
        return;
    }
    loop {
        match read_packet(&mut reader) {
            Some(Packet::Flush) => break,
            Some(Packet::Data(_)) => {}
            _ => return,
        }
    }
    if sent.send(String::new()).is_err() {
        return;
    }
    let mut found = String::new();
    loop {
        match read_packet(&mut reader) {
            Some(Packet::End) => {
                if sent.send(std::mem::take(&mut found)).is_err() {
                    return;
                }
            }
            Some(Packet::Data(data)) => {
                let text = String::from_utf8_lossy(&data);
                let mut words = text.split_whitespace();
                if let (Some(oid), Some(SOURCE_REF)) = (words.next(), words.next()) {
                    oid.clone_into(&mut found);
                }
            }
            Some(Packet::Flush | Packet::Delim) => {}
            None => return,
        }
    }
}

fn current_runtime() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from);
    fs::canonicalize(state.join("north/current")).ok()
}
