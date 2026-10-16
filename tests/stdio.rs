use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};

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
    let script = r#"
process.stdout.write("");
process.stderr.write("");
const child = Bun.spawn([process.env.STDIO_NORTH, "fleet", "list", "--json"], {
    stdin: "ignore", stdout: "inherit", stderr: "inherit",
});
process.stderr.write(child.pid + "\n");
process.exitCode = await child.exited;
"#;
    let mut process = Process(Command::new("bun").args(["-e", script])
        .env("STDIO_NORTH", env!("CARGO_BIN_EXE_north"))
        .env("HOME", scratch.path())
        .env("NORTH_FLEET_SESSION", "")
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap());
    let mut errors = BufReader::new(process.0.stderr.take().unwrap());
    let mut receipt = String::new();
    errors.read_line(&mut receipt).unwrap();
    let pid: u32 = receipt.trim().parse().unwrap();
    let mut output = process.0.stdout.take().unwrap();
    let mut first = [0];
    output.read_exact(&mut first).unwrap();
    for descriptor in [1, 2] {
        assert!(nonblocking(process.0.id(), descriptor), "the parent must keep its own descriptor flags");
        assert!(!nonblocking(pid, descriptor), "the CLI must own blocking output descriptors");
    }
    let mut text = first.to_vec();
    output.read_to_end(&mut text).unwrap();
    assert!(process.0.wait().unwrap().success());
    let records: Vec<serde_json::Value> = serde_json::from_slice(&text).unwrap();
    assert_eq!(records[0]["name"], name);
}
