use std::fs;
use std::io::{PipeReader, PipeWriter, Read};
use std::process::{Child, Command, Stdio};

use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn nonblocking(pid: u32, descriptor: u8) -> bool {
    let info = fs::read_to_string(format!("/proc/{pid}/fdinfo/{descriptor}")).expect("CLI exited before its output was drained");
    let flags = info.lines().find_map(|line| line.strip_prefix("flags:\t")).unwrap();
    u32::from_str_radix(flags, 8).unwrap() & 0o4000 != 0
}

fn nonblocking_pipe() -> (PipeReader, PipeWriter) {
    let (reader, writer) = std::io::pipe().unwrap();
    fcntl_setfl(&writer, fcntl_getfl(&writer).unwrap() | OFlags::NONBLOCK).unwrap();
    (reader, writer)
}

#[test]
fn cli_output_survives_backpressure_from_a_nonblocking_parent() {
    let scratch = tempfile::tempdir().unwrap();
    let session = scratch.path().join(".local/state/north/fleet/fixture");
    fs::create_dir_all(&session).unwrap();
    let name = "n".repeat(1 << 20);
    fs::write(session.join("session.json"), serde_json::to_vec(&serde_json::json!({
        "name": name, "provider": "claude", "account": "fixture", "cwd": scratch.path(),
        "model": null, "created_ms": 1, "host_pid": null, "conversation": null
    })).unwrap()).unwrap();
    let (mut output, output_writer) = nonblocking_pipe();
    let (_errors, errors_writer) = nonblocking_pipe();
    let mut process = Process(Command::new(env!("CARGO_BIN_EXE_north")).args(["fleet", "list", "--json"])
        .env("HOME", scratch.path())
        .env("NORTH_FLEET_SESSION", "")
        .stdin(Stdio::null())
        .stdout(output_writer.try_clone().unwrap())
        .stderr(errors_writer.try_clone().unwrap())
        .spawn().unwrap());
    let mut first = [0];
    output.read_exact(&mut first).unwrap();
    for (descriptor, writer) in [(1, &output_writer), (2, &errors_writer)] {
        assert!(fcntl_getfl(writer).unwrap().contains(OFlags::NONBLOCK), "the parent must keep its own descriptor flags");
        assert!(!nonblocking(process.0.id(), descriptor), "the CLI must own blocking output descriptors");
    }
    drop((output_writer, errors_writer));
    let mut text = first.to_vec();
    output.read_to_end(&mut text).unwrap();
    assert!(process.0.wait().unwrap().success());
    let records: Vec<serde_json::Value> = serde_json::from_slice(&text).unwrap();
    assert_eq!(records[0]["name"], name);
}
