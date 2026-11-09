use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROXY_VARIABLES: [&str; 8] = ["http_proxy", "https_proxy", "all_proxy", "no_proxy", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"];

pub fn run(arguments: &[&str], config: &str, limit: Duration) -> Option<Vec<u8>> {
    let mut command = Command::new("curl");
    command.arg("-q").args(["--silent", "--show-error", "--globoff"]).args(arguments).args(["--config", "-"]);
    PROXY_VARIABLES.iter().for_each(|name| {
        command.env_remove(name);
    });
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let mut stdout = child.stdout.take()?;
    let input = config.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stdout.read_to_end(&mut output).ok().map(|_| output)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let _ = writer.join();
    let output = reader.join().ok().flatten();
    status.filter(std::process::ExitStatus::success).and(output)
}

pub fn quoted(text: &str) -> Result<String, String> {
    if text.bytes().any(|byte| byte == 0) {
        return Err("a request header or body holds a NUL".to_owned());
    }
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    Ok(out)
}
