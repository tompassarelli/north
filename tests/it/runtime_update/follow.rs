use super::harness::*;
use north_core::foundation::shim;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::{Child, Command};
use std::time::Duration;

const RUNTIMES: [&str; 3] = ["8917932b11111111111111111111111111111111", "cb929a45e8222222222222222222222222222222", "9a896708b01683bd4523100a6c79fa0562e34aac"];

fn spawn(command: &mut Command) -> Child {
    loop {
        match command.spawn() {
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::yield_now(),
            spawned => break spawned.unwrap(),
        }
    }
}

fn promote(host: &Host, sha: &str) {
    let link = host.path("state/north/current");
    let _ = fs::remove_file(&link);
    symlink(Path::new("runtimes").join(sha), link).unwrap();
}

#[test]
fn a_fleet_watchdog_moves_onto_each_promoted_runtime_in_place_and_keeps_watching_its_session() {
    let host = Host::new();
    for sha in RUNTIMES {
        host.built(sha);
        shim::copy_executable(crate::support::bin_exe!("CARGO_BIN_EXE_north"), host.runtime(sha).join("north"));
    }
    promote(&host, RUNTIMES[0]);
    let mut session_host = spawn(Command::new("sleep").arg("600"));
    let session = host.path("state/north/fleet/worker-1");
    fs::create_dir_all(&session).unwrap();
    let record = serde_json::json!({"name": "worker-1", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": session_host.id(), "role": "worker"});
    fs::write(session.join("session.json"), record.to_string()).unwrap();
    let north = |sha: &str, args: &[&str]| {
        let mut command = Command::new(host.runtime(sha).join("north"));
        command.args(args).env("HOME", host.path("home")).env("XDG_STATE_HOME", host.path("state")).env("THREADS_DB", host.path("threads.db")).env_remove("NORTH_RUNTIME_LINK").env_remove("NORTH_FLEET_SESSION");
        command
    };
    let mut watchdog = spawn(north(RUNTIMES[0], &["fleet", "watchdog", "worker-1"]).stdout(std::process::Stdio::null()));
    let pid = watchdog.id();
    let alive = |watchdog: &mut Child| watchdog.try_wait().unwrap().is_none();
    let lag = || {
        let text = String::from_utf8_lossy(&north(RUNTIMES[2], &["runtime", "lag", "--processes"]).output().unwrap().stdout).into_owned();
        text.lines().find(|line| line.starts_with(&format!("{pid} "))).unwrap_or_default().to_owned()
    };
    for sha in &RUNTIMES[1..] {
        promote(&host, sha);
        let moved = north_core::platform::wait::until(Duration::from_secs(10), || lag().ends_with(&format!("runs {}: current", &sha[..10])) || !alive(&mut watchdog));
        assert!(moved && alive(&mut watchdog), "10 s after current moved to {sha}, runtime lag --processes reads the watchdog as {:?}", lag());
    }
    session_host.kill().unwrap();
    session_host.wait().unwrap();
    assert!(north_core::platform::wait::until(Duration::from_secs(20), || !alive(&mut watchdog)), "the moved watchdog never acted on its session host's death");
}

fn frame(socket: &Path, n: usize) -> String {
    use std::io::{BufRead, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    writeln!(stream, r#"{{"type":"user","n":{n}}}"#).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    std::io::BufReader::new(stream).lines().next().and_then(Result::ok).unwrap_or_default()
}

#[test]
fn a_session_host_moves_onto_each_promoted_runtime_in_place_and_keeps_its_provider_and_sequence() {
    let host = Host::new();
    for sha in RUNTIMES {
        host.built(sha);
        shim::copy_executable(crate::support::bin_exe!("CARGO_BIN_EXE_north"), host.runtime(sha).join("north"));
    }
    promote(&host, RUNTIMES[0]);
    let (state, socket) = (host.path("state/north/fleet/worker-1"), host.path("worker-1.sock"));
    let north = |sha: &str, args: &[&str]| {
        let mut command = Command::new(host.runtime(sha).join("north"));
        command.args(args).env("HOME", host.path("home")).env("XDG_STATE_HOME", host.path("state")).env("THREADS_DB", host.path("threads.db")).env_remove("NORTH_RUNTIME_LINK").env_remove("NORTH_FLEET_SESSION");
        command
    };
    let (state_dir, socket_path) = (state.display().to_string(), socket.display().to_string());
    let arguments = ["session-host", "--name", "worker-1", "--generation", "1", "--state-dir", &state_dir, "--socket", &socket_path, "--", "sh", "-c", r#"echo "{\"provider\":$$}"; exec cat"#];
    let errors = fs::File::create(host.path("session-host.err")).unwrap();
    let mut session_host = spawn(north(RUNTIMES[0], &arguments).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(errors));
    let pid = session_host.id();
    let lag = || {
        let text = String::from_utf8_lossy(&north(RUNTIMES[2], &["runtime", "lag", "--processes"]).output().unwrap().stdout).into_owned();
        text.lines().find(|line| line.starts_with(&format!("{pid} "))).unwrap_or_default().to_owned()
    };
    assert!(north_core::platform::wait::until(Duration::from_secs(10), || socket.exists()), "the session host never bound its socket");
    assert_eq!(frame(&socket, 1), r#"{"host_seq":1}"#);
    for (n, sha) in RUNTIMES[1..].iter().enumerate() {
        promote(&host, sha);
        let moved = north_core::platform::wait::until(Duration::from_secs(15), || lag().ends_with(&format!("runs {}: current", &sha[..10])) || session_host.try_wait().unwrap().is_some());
        let said = fs::read_to_string(host.path("session-host.err")).unwrap_or_default();
        assert!(moved && session_host.try_wait().unwrap().is_none(), "15 s after current moved to {sha}, runtime lag --processes reads the session host as {:?}; it said {said:?}", lag());
        assert_eq!(frame(&socket, n + 2), format!(r#"{{"host_seq":{}}}"#, n + 2), "the moved session host lost its sequence; it said {said:?}");
    }
    let stream = state.join("g1/stream.jsonl");
    let delivered = || fs::read_to_string(&stream).unwrap_or_default();
    assert!(north_core::platform::wait::until(Duration::from_secs(10), || delivered().lines().count() >= 4), "the provider echoed {:?}", delivered());
    let lines: Vec<String> = delivered().lines().map(String::from).collect();
    assert!(lines[0].starts_with(r#"{"provider":"#) && lines.iter().filter(|line| line.contains("provider")).count() == 1, "the provider restarted: {lines:?}");
    assert_eq!(lines[1..], [1, 2, 3].map(|n| format!(r#"{{"type":"user","n":{n}}}"#)), "the frames sent across both moves reached the one provider in order");
    session_host.kill().unwrap();
    session_host.wait().unwrap();
}

#[test]
fn a_tool_server_answers_each_call_from_the_runtime_current_names_and_answers_none_twice() {
    use std::io::{BufRead, BufReader, Write};
    let host = Host::new();
    for sha in RUNTIMES {
        host.built(sha);
        shim::copy_executable(crate::support::bin_exe!("CARGO_BIN_EXE_north"), host.runtime(sha).join("north"));
    }
    promote(&host, RUNTIMES[0]);
    let north = |sha: &str, args: &[&str]| {
        let mut command = Command::new(host.runtime(sha).join("north"));
        command.args(args).env("HOME", host.path("home")).env("XDG_STATE_HOME", host.path("state")).env("THREADS_DB", host.path("threads.db")).env_remove("NORTH_RUNTIME_LINK").env_remove("NORTH_FLEET_SESSION");
        command
    };
    let mut server = spawn(north(RUNTIMES[0], &["--tool-server", "runtime"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()));
    let pid = server.id();
    let mut requests = server.stdin.take().unwrap();
    let mut answers = BufReader::new(server.stdout.take().unwrap());
    let mut call = |id: u32| {
        let request = serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": "runtime", "arguments": {"arguments": ["lag", "--processes"]}}});
        writeln!(requests, "{request}").unwrap();
        let mut line = String::new();
        answers.read_line(&mut line).unwrap();
        let answer: serde_json::Value = serde_json::from_str(&line).unwrap_or_else(|error| panic!("call {id} got {line:?}: {error}"));
        assert_eq!(answer["id"], id);
        let text = answer["result"]["content"][0]["text"].as_str().unwrap_or_default().to_owned();
        text.lines().find(|line| line.starts_with(&format!("{pid} "))).unwrap_or_default().to_owned()
    };
    assert_eq!(call(1), format!("{pid} tool-server runtime runs {}: current", &RUNTIMES[0][..10]));
    promote(&host, RUNTIMES[1]);
    assert_eq!(call(2), format!("{pid} tool-server runtime runs {}: current", &RUNTIMES[1][..10]), "a call sent after current moved was answered from the old runtime");
    promote(&host, RUNTIMES[2]);
    let lag = || {
        let text = String::from_utf8_lossy(&north(RUNTIMES[2], &["runtime", "lag", "--processes"]).output().unwrap().stdout).into_owned();
        text.lines().find(|line| line.starts_with(&format!("{pid} "))).unwrap_or_default().to_owned()
    };
    assert!(north_core::platform::wait::until(Duration::from_secs(10), || lag().ends_with(&format!("runs {}: current", &RUNTIMES[2][..10]))), "10 s after current moved, the idle tool server reads {:?}", lag());
    assert_eq!(call(3), format!("{pid} tool-server runtime runs {}: current", &RUNTIMES[2][..10]));
    drop(requests);
    let mut rest = String::new();
    answers.read_line(&mut rest).unwrap();
    assert_eq!(rest, "", "a call was answered twice");
    assert!(server.wait().unwrap().success());
}
