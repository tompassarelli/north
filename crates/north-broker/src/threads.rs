use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::policy::{Caller, name_ok};

const MAX_ARGUMENTS: usize = 64;
const MAX_WORD: usize = 8 * 1024;
const MAX_INPUT: usize = 1 << 20;
const MAX_OUTPUT: u64 = 1 << 20;
const DEADLINE: Duration = Duration::from_secs(60);

/// The ledger forward: runs `north threads VERB WORDS` as the broker's own user for a caller whose policy names the verb, so the ledger host's ssh login stays the operator's.
pub struct Threads {
    program: PathBuf,
}

type Typed = (String, Vec<String>, String);

impl Threads {
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }

    /// The typed request `{verb, arguments, stdin}` as the command a caller's policy allows, or why it is refused: the verb must be one the policy lists, and a word that names a path is refused because the operator would read it.
    fn typed(caller: &Caller, body: &Value) -> Result<Typed, (u16, String)> {
        let verb = body["verb"].as_str().unwrap_or_default();
        if !name_ok(verb) || !caller.threads.iter().any(|allowed| allowed == verb) {
            return Err((403, format!("this caller may not run threads {verb:?}")));
        }
        let words: Option<Vec<String>> = body["arguments"].as_array().map_or(Some(Vec::new()), |words| words.iter().map(|word| word.as_str().map(str::to_owned)).collect());
        let words = words.ok_or((400, "arguments are not a list of strings".to_owned()))?;
        if words.len() > MAX_ARGUMENTS || words.iter().any(|word| word.len() > MAX_WORD || word.contains('\0')) {
            return Err((400, "arguments are too many, too long or hold a NUL".to_owned()));
        }
        if let Some(word) = words.iter().find(|word| word.starts_with('/') || word.starts_with('~') || word.contains("..")) {
            return Err((403, format!("{word:?} names a path; threads requests carry values, not files")));
        }
        let input = body["stdin"].as_str().unwrap_or_default().to_owned();
        if input.len() > MAX_INPUT {
            return Err((400, "stdin is too long".to_owned()));
        }
        Ok((verb.to_owned(), words, input))
    }

    pub fn forward(&self, caller: &Caller, body: &Value) -> Result<Value, (u16, String)> {
        let (verb, words, input) = Self::typed(caller, body)?;
        let mut child = Command::new(&self.program)
            .args(["threads", &verb])
            .args(&words)
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| (502, format!("{}: {error}", self.program.display())))?;
        let pid = child.id();
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
        let (done, waited) = mpsc::channel();
        std::thread::spawn(move || {
            let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
            let errors = std::thread::spawn(move || {
                let mut text = Vec::new();
                if let Some(stream) = stderr {
                    let _ = stream.take(MAX_OUTPUT).read_to_end(&mut text);
                }
                text
            });
            let mut text = Vec::new();
            if let Some(stream) = stdout {
                let _ = stream.take(MAX_OUTPUT).read_to_end(&mut text);
            }
            let errors = errors.join().unwrap_or_default();
            let code = child.wait().ok().and_then(|status| status.code());
            let _ = done.send((code, text, errors));
        });
        match waited.recv_timeout(DEADLINE) {
            Ok((code, out, err)) => Ok(json!({ "code": code.unwrap_or(1), "stdout": String::from_utf8_lossy(&out), "stderr": String::from_utf8_lossy(&err) })),
            Err(_) => {
                if let Some(pid) = i32::try_from(pid).ok().and_then(rustix::process::Pid::from_raw) {
                    let _ = rustix::process::kill_process(pid, rustix::process::Signal::Kill);
                }
                Err((504, "the ledger forward did not finish in time".to_owned()))
            }
        }
    }
}
