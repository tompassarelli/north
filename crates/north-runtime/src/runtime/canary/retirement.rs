//! A promotion retires the on-demand host services it would otherwise fail on. A client starts one (the user broker, a
//! GitHub broker no unit keeps) and nothing restarts it, so one left on a runtime that is neither live nor judged here
//! serves that runtime until it exits. It is asked to exit at its next idle moment: its socket path goes first, so the
//! next client starts one on the live runtime, and it gets a TERM once no client has been connected for a quiet spell.
//! Its log tells nothing of that: the one its next client starts writes to the same file. One still serving a client is
//! named as draining, since no new client reaches it, and one whose path could not be taken away as still accepting.
use super::super::layout;
use north_runtime_follow::runtime_of;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::time::{Duration, Instant};

/// Each kind a client starts on demand: the words its command begins with, and the flag whose presence or absence tells it from the one a unit keeps.
const ON_DEMAND: [(&str, [&str; 2], &str, bool); 2] = [("broker", ["broker", "serve"], "--user", true), ("gh-broker", ["gh-broker", "serve"], "--resident", false)];
const QUIET: Duration = Duration::from_secs(5);
const PATIENCE: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(200);
/// `/proc/net/unix` marks a listening socket with __SO_ACCEPTCON in its Flags column.
const LISTENS: u32 = 0x10000;

#[derive(Deserialize)]
struct Process {
    pid: u32,
    #[serde(default)]
    kind: String,
    runtime: Option<String>,
}

struct Stale {
    pid: u32,
    kind: String,
    runtime: String,
}

impl Stale {
    fn named(&self) -> String {
        format!("`{}` (pid {}) on {}", self.kind, self.pid, layout::short(&self.runtime))
    }
}

struct Socket {
    inode: u64,
    listens: bool,
    path: Option<String>,
}

/// What a promotion asked of the on-demand host services behind it: a line on each, and the ones still running that no new client reaches.
#[derive(Default)]
pub(super) struct Asked {
    pub(super) said: Vec<String>,
    pub(super) draining: Vec<u32>,
}

pub(super) struct Host<'a> {
    pub(super) proc_root: &'a Path,
    pub(super) quiet: Duration,
    pub(super) patience: Duration,
    pub(super) term: &'a dyn Fn(u32),
}

impl Host<'_> {
    /// Whether the process is still the on-demand service the listing named, on the runtime it named: a pid can be taken again between the listing and the signal.
    fn serves(&self, stale: &Stale) -> bool {
        let process = self.proc_root.join(stale.pid.to_string());
        let Ok(cmdline) = fs::read(process.join("cmdline")) else {
            return false;
        };
        let argv: Vec<String> = cmdline.split(|byte| *byte == 0).filter(|word| !word.is_empty()).map(|word| String::from_utf8_lossy(word).into_owned()).collect();
        let Some(program) = argv.first() else {
            return false;
        };
        let rest: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
        let on_demand = ON_DEMAND.iter().any(|(kind, words, flag, flagged)| stale.kind == *kind && rest.starts_with(words) && rest.contains(flag) == *flagged);
        let built = |path: &Path| runtime_of(path).map(|(runtime, _)| runtime);
        on_demand && built(Path::new(program)).or_else(|| fs::read_link(process.join("exe")).ok().and_then(|exe| built(&exe))).as_deref() == Some(stale.runtime.as_str())
    }

    fn held(&self, pid: u32) -> Option<HashSet<u64>> {
        let open = fs::read_dir(self.proc_root.join(pid.to_string()).join("fd")).ok()?;
        Some(open.flatten().filter_map(|fd| fs::read_link(fd.path()).ok()?.to_str()?.strip_prefix("socket:[")?.strip_suffix(']')?.parse().ok()).collect())
    }

    fn sockets(&self) -> Vec<Socket> {
        let table = fs::read_to_string(self.proc_root.join("net/unix")).unwrap_or_default();
        table.lines().filter_map(|line| {
            let mut columns = line.split_whitespace();
            let flags = u32::from_str_radix(columns.nth(3)?, 16).ok()?;
            Some(Socket { inode: columns.nth(2)?.parse().ok()?, listens: flags & LISTENS != 0, path: columns.next().map(str::to_owned) })
        }).collect()
    }

    /// The paths at which a new client still reaches the service: a name outside the filesystem, or a socket file that is its own; a path another listener has bound since is that listener's.
    fn reached_at(&self, pid: u32) -> Option<Vec<String>> {
        let (held, sockets) = (self.held(pid)?, self.sockets());
        let own = |path: &str| !path.starts_with('/') || (sockets.iter().filter(|socket| socket.listens && socket.path.as_deref() == Some(path)).count() == 1 && fs::symlink_metadata(path).is_ok_and(|file| file.file_type().is_socket()));
        Some(sockets.iter().filter(|socket| socket.listens && held.contains(&socket.inode)).filter_map(|socket| socket.path.clone()).filter(|path| own(path)).collect())
    }

    /// Takes the service's socket path away, so no new client reaches it.
    fn close(&self, pid: u32) {
        for path in self.reached_at(pid).unwrap_or_default().iter().filter(|path| path.starts_with('/')) {
            let _ = fs::remove_file(path);
        }
    }

    /// A process whose sockets cannot be read is taken as reachable and as serving a client.
    fn accepts(&self, pid: u32) -> bool {
        self.reached_at(pid).is_none_or(|paths| !paths.is_empty())
    }

    fn connected(&self, pid: u32) -> bool {
        self.held(pid).is_none_or(|held| self.sockets().iter().any(|socket| !socket.listens && held.contains(&socket.inode)))
    }
}

fn term(pid: u32) {
    if let Some(pid) = i32::try_from(pid).ok().and_then(rustix::process::Pid::from_raw) {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::Term);
    }
}

pub(super) fn retire_here(live: Option<&str>, judged: &str, listing: &str) -> Asked {
    retire(live, judged, listing, &Host { proc_root: Path::new("/proc"), quiet: QUIET, patience: PATIENCE, term: &term })
}

/// Asks each on-demand host service the listing shows on a runtime other than `live` and `judged` to exit once idle, and says what became of each.
pub(super) fn retire(live: Option<&str>, judged: &str, listing: &str, host: &Host) -> Asked {
    let listed: Vec<Process> = serde_json::from_str(listing.trim()).unwrap_or_default();
    let older = listed.into_iter().filter_map(|process| {
        let runtime = process.runtime.filter(|runtime| runtime != judged && Some(runtime.as_str()) != live)?;
        Some(Stale { pid: process.pid, kind: process.kind, runtime })
    });
    let mut serving: Vec<Stale> = older.filter(|stale| host.serves(stale)).collect();
    if serving.is_empty() {
        return Asked::default();
    }
    serving.iter().for_each(|stale| host.close(stale.pid));
    let (started, mut clientless, mut asked, mut said) = (Instant::now(), HashMap::new(), Vec::new(), Vec::new());
    loop {
        std::thread::sleep(POLL);
        serving.retain(|stale| host.serves(stale));
        for stale in &serving {
            if host.connected(stale.pid) {
                clientless.remove(&stale.pid);
            } else {
                clientless.entry(stale.pid).or_insert_with(Instant::now);
            }
        }
        let (idle, busy): (Vec<Stale>, Vec<Stale>) = serving.into_iter().partition(|stale| clientless.get(&stale.pid).is_some_and(|since| since.elapsed() >= host.quiet));
        idle.iter().for_each(|stale| (host.term)(stale.pid));
        asked.extend(idle);
        serving = busy;
        if serving.is_empty() || started.elapsed() >= host.patience {
            break;
        }
    }
    while asked.iter().any(|stale| host.serves(stale)) && started.elapsed() < host.patience {
        std::thread::sleep(POLL);
    }
    said.extend(asked.iter().map(|stale| format!("asked {} to exit while it served no client; its next client starts it on the live runtime", stale.named())));
    let (accepting, draining): (Vec<&Stale>, Vec<&Stale>) = serving.iter().chain(asked.iter().filter(|stale| host.serves(stale))).partition(|stale| host.accepts(stale.pid));
    said.extend(draining.iter().filter(|stale| serving.iter().any(|busy| busy.pid == stale.pid)).map(|stale| format!("{} is serving a client and takes no new one; the next promotion asks it to exit again", stale.named())));
    said.extend(accepting.iter().map(|stale| format!("{} still takes new clients, since its socket path could not be taken away", stale.named())));
    Asked { said, draining: draining.iter().map(|stale| stale.pid).collect() }
}

#[cfg(test)]
mod tests {
    use super::super::super::layout::{self, Layout};
    use super::super::services::judge;
    use super::{Host, retire};
    use std::cell::RefCell;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::time::Duration;

    const OLDER: &str = "0000000000000000000000000000000000000000";
    const REPLACED: &str = "1111111111111111111111111111111111111111";
    const LIVE: &str = "2222222222222222222222222222222222222222";

    fn process(proc_root: &Path, pid: u32, runtime: &str, words: &[&str], sockets: &[u64]) {
        let process = proc_root.join(pid.to_string());
        fs::create_dir_all(process.join("fd")).unwrap();
        fs::write(process.join("cmdline"), std::iter::once(format!("/state/north/runtimes/{runtime}/north")).chain(words.iter().map(|word| (*word).to_owned())).map(|word| word + "\0").collect::<String>()).unwrap();
        for (fd, inode) in sockets.iter().enumerate() {
            symlink(format!("socket:[{inode}]"), process.join("fd").join((fd + 3).to_string())).unwrap();
        }
    }

    #[test]
    fn a_user_broker_on_an_older_runtime_is_asked_to_exit_only_once_no_client_is_connected() {
        let root = tempfile::tempdir().unwrap();
        let (proc_root, socket) = (root.path().join("proc"), root.path().join("broker.sock"));
        let _bound = UnixListener::bind(&socket).unwrap();
        process(&proc_root, 7, OLDER, &["broker", "serve", "--user"], &[100, 101]);
        process(&proc_root, 8, OLDER, &["broker", "serve", "--socket", "/run/north-broker/broker.sock", "--policy", "/etc/north/broker.toml"], &[102]);
        process(&proc_root, 9, LIVE, &["broker", "serve", "--user"], &[103]);
        fs::create_dir_all(proc_root.join("net")).unwrap();
        let table = format!("Num       RefCount Protocol Flags    Type St Inode Path\n0000000000000000: 00000002 00000000 00010000 0001 01 100 {}\n0000000000000000: 00000003 00000000 00000000 0001 03 101 {}\n0000000000000000: 00000002 00000000 00010000 0001 01 102 /run/north-broker/broker.sock\n", socket.display(), socket.display());
        fs::write(proc_root.join("net/unix"), table).unwrap();
        let listing = serde_json::json!([{"pid": 7, "kind": "broker", "process": "broker", "runtime": OLDER}, {"pid": 8, "kind": "broker", "process": "broker /run/north-broker/broker.sock", "runtime": OLDER}, {"pid": 9, "kind": "broker", "process": "broker", "runtime": LIVE}, {"pid": 10, "kind": "session-host", "process": "session-host worker-1", "runtime": OLDER}]).to_string();
        let asked = RefCell::new(Vec::new());
        let exits = |pid: u32| {
            asked.borrow_mut().push(pid);
            fs::remove_dir_all(proc_root.join(pid.to_string())).unwrap();
        };
        let host = Host { proc_root: &proc_root, quiet: Duration::ZERO, patience: Duration::ZERO, term: &exits };

        let said = retire(Some(LIVE), LIVE, &listing, &host).said;
        assert!(asked.borrow().is_empty(), "a broker with a client connected was signalled mid-request: {said:?}");
        assert!(!socket.exists(), "the older broker kept its socket path, so its next client would reach it again");
        assert!(said.len() == 1 && said[0].contains("`broker` (pid 7) on 00000000 is serving a client"), "{said:?}");

        fs::remove_file(proc_root.join("7/fd/4")).unwrap();
        let said = retire(Some(LIVE), LIVE, &listing, &host).said;
        assert_eq!(*asked.borrow(), [7], "only the idle on-demand broker on the older runtime is asked to exit, not the unit's broker or the live one: {said:?}");
        assert!(said.len() == 1 && said[0].contains("asked `broker` (pid 7) on 00000000 to exit"), "{said:?}");
    }

    #[test]
    fn a_stale_broker_no_new_client_reaches_is_named_and_passes_the_probe_and_one_that_still_accepts_fails_it() {
        let root = tempfile::tempdir().unwrap();
        let (proc_root, socket) = (root.path().join("proc"), root.path().join("broker.sock"));
        let _bound = UnixListener::bind(&socket).unwrap();
        process(&proc_root, 7, OLDER, &["broker", "serve", "--user"], &[100, 101]);
        process(&proc_root, 8, OLDER, &["broker", "serve", "--user"], &[102, 103]);
        fs::create_dir_all(proc_root.join("net")).unwrap();
        let table = format!("Num       RefCount Protocol Flags    Type St Inode Path\n0000000000000000: 00000002 00000000 00010000 0001 01 100 {}\n0000000000000000: 00000003 00000000 00000000 0001 03 101 {}\n0000000000000000: 00000002 00000000 00010000 0001 01 102 @north-broker\n0000000000000000: 00000003 00000000 00000000 0001 03 103 @north-broker\n", socket.display(), socket.display());
        fs::write(proc_root.join("net/unix"), table).unwrap();
        let layout = Layout { state: root.path().to_path_buf() };
        fs::write(layout.swaps(), [OLDER, REPLACED, LIVE].iter().enumerate().map(|(at, runtime)| format!("{{\"runtime\":\"{runtime}\",\"at\":{at}}}\n")).collect::<String>()).unwrap();
        symlink(layout::link(LIVE), layout.current()).unwrap();
        let host = Host { proc_root: &proc_root, quiet: Duration::ZERO, patience: Duration::ZERO, term: &|pid| panic!("pid {pid} was signalled while it served a client") };
        let broker = |pid: u32| serde_json::json!({"pid": pid, "kind": "broker", "process": "broker", "runtime": OLDER});

        let listing = serde_json::json!([broker(7)]).to_string();
        let asked = retire(Some(LIVE), LIVE, &listing, &host);
        assert!(asked.said.len() == 1 && asked.said[0].contains("`broker` (pid 7) on 00000000 is serving a client and takes no new one"), "{:?}", asked.said);
        let passed = judge(&layout, LIVE, &listing, &asked.draining).map_err(|failure| failure.message);
        assert!(passed.as_ref().is_ok_and(|said| said.contains("`broker` (pid 7) on 00000000 takes no new client and leaves with its last one")), "a broker no new client reaches held the host on its old runtime: {passed:?}");

        let listing = serde_json::json!([broker(7), broker(8)]).to_string();
        let asked = retire(Some(LIVE), LIVE, &listing, &host);
        assert!(asked.said.iter().any(|line| line.contains("`broker` (pid 8) on 00000000 still takes new clients")), "{:?}", asked.said);
        let failed = judge(&layout, LIVE, &listing, &asked.draining).map_err(|failure| failure.message);
        assert!(failed.as_ref().is_err_and(|why| why.contains("`broker` (pid 8) runs 00000000") && !why.contains("pid 7")), "a stale broker that still takes new clients passed, or the draining one failed with it: {failed:?}");
    }
}
