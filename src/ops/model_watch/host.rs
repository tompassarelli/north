use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::models::Source;
use crate::foundation::clock::Timestamp;

pub(super) struct Finished {
    pub(super) code: i32,
    pub(super) stdout: String,
}

pub(super) trait Host {
    fn fetch(&self, source: &Source, timeout: f64) -> Result<Vec<u8>, String>;
    fn command(&self, program: &str, args: &[String], input: Option<&str>, timeout: Duration, env: &[(&str, &str)]) -> Result<Finished, String>;
    fn now(&self) -> Timestamp;
    fn account(&self) -> Option<String>;
}

pub(super) struct Real;

const LIMIT: usize = 8_000_000;

impl Host for Real {
    fn fetch(&self, source: &Source, timeout: f64) -> Result<Vec<u8>, String> {
        let args: Vec<String> = ["-sS", "-L", "--max-time", &timeout.to_string(), "-A", "model-watch/1.0", "-w", "\n%{http_code}", &source.endpoint].map(str::to_owned).to_vec();
        let done = self.command_bytes("curl", &args, None, Duration::from_secs_f64(timeout + 5.0), &[])?;
        let split = done.1.iter().rposition(|byte| *byte == b'\n').ok_or("OSError")?;
        let code = std::str::from_utf8(&done.1[split + 1..]).unwrap_or("");
        if done.0 != 0 {
            return Err("OSError".into());
        }
        if !code.starts_with('2') {
            return Err(format!("HTTP {code}"));
        }
        if split > LIMIT {
            return Err("source exceeds size limit".into());
        }
        Ok(done.1[..split].to_vec())
    }

    fn command(&self, program: &str, args: &[String], input: Option<&str>, timeout: Duration, env: &[(&str, &str)]) -> Result<Finished, String> {
        let (code, stdout) = self.command_bytes(program, args, input.map(str::as_bytes), timeout, env)?;
        Ok(Finished { code, stdout: String::from_utf8_lossy(&stdout).into_owned() })
    }

    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn account(&self) -> Option<String> {
        claude_account()
    }
}

impl Real {
    fn command_bytes(&self, program: &str, args: &[String], input: Option<&[u8]>, timeout: Duration, env: &[(&str, &str)]) -> Result<(i32, Vec<u8>), String> {
        let mut command = Command::new(program);
        command.args(args).envs(env.iter().copied()).stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| "OSError".to_owned())?;
        let writer = child.stdin.take().zip(input.map(<[u8]>::to_vec)).map(|(mut stdin, bytes)| thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        }));
        let mut stdout = child.stdout.take().ok_or("OSError")?;
        let reader = thread::spawn(move || {
            let mut collected = Vec::new();
            let _ = stdout.read_to_end(&mut collected);
            collected
        });
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|_| "OSError".to_owned())? {
                break status;
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err("TimeoutExpired".into());
            }
            thread::sleep(Duration::from_millis(20));
        };
        if let Some(writer) = writer {
            let _ = writer.join();
        }
        Ok((status.code().unwrap_or(-1), reader.join().unwrap_or_default()))
    }
}

pub(super) fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(path),
    }
}

pub(super) fn shell_quote(text: &str) -> String {
    let safe = |byte: u8| byte.is_ascii_alphanumeric() || b"@%+=:,./-_".contains(&byte);
    if text.is_empty() {
        "''".into()
    } else if text.bytes().all(safe) {
        text.into()
    } else {
        format!("'{}'", text.replace('\'', "'\"'\"'"))
    }
}

fn claude_account() -> Option<String> {
    use sha2::{Digest, Sha256};
    let home = std::env::var("HOME").unwrap_or_default();
    let directory = std::env::var("CLAUDE_CONFIG_DIR").ok().filter(|value| !value.is_empty()).unwrap_or(home.clone());
    let cache: serde_json::Value = serde_json::from_str(&fs::read_to_string(PathBuf::from(directory).join(".claude.json")).ok()?).ok()?;
    let uuid = [&cache["oauthAccount"]["accountUuid"], &cache["cachedUsageUtilization"]["accountUuid"]].into_iter().find_map(|uuid| uuid.as_str().filter(|uuid| !uuid.is_empty()))?;
    let identity = format!("{:x}", Sha256::digest(uuid.as_bytes()))[..8].to_owned();
    let operator = std::env::var("AGENTS_OPERATOR_DIR").ok().filter(|value| !value.is_empty()).map_or_else(|| expand_home("~/code/south/main/agents"), PathBuf::from);
    let policy = std::env::var("AGENTS_ORCHESTRATION").ok().filter(|value| !value.is_empty()).map_or_else(|| operator.join("orchestration.toml"), PathBuf::from);
    let document = fs::read_to_string(policy).ok()?.parse::<toml_edit::DocumentMut>().ok()?;
    document.get("accounts")?.get("claude")?.get("labels")?.get(&identity)?.as_str().map(str::to_owned)
}
