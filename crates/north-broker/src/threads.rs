use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
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

/// Every flag a `north threads` verb declares in north-threads' argument parsers, valued or switch; the broker checks each word against this list, because the parsers are not exported.
const FLAGS: &[&str] = &[
    "--by", "--since", "--launched", "--session", "--source", "--cause", "--reason", "--settled", "--ended", "--capacity", "--repo", "--candidate", "--tree", "--base", "--batch-size", "--bisect-depth", "--started-at", "--finished-at", "--outcome", "--command", "--deadline", "--sha", "--red-at", "--green-at", "--cases", "--revert-sha", "--lane", "--branch", "--tip", "--queued-at", "--attempts", "--landed-at", "--landed-sha", "--verdict", "--detail", "--first-commit-at", "--check-started-at", "--check-finished-at", "--at", "--window-minutes", "--landed", "--first-commit-samples", "--refused-alone", "--conflicts", "--queued-now", "--queued-p50", "--queued-p90", "--first-commit-p50", "--first-commit-p90", "--tier", "--parent", "--host", "--started", "--finished", "--rc", "--json", "--to", "--held-by", "--box", "--due", "--body", "--clear", "--key", "--class", "--noticed-by", "--subject", "--detected-at", "--guard", "--issue", "--days", "--until",
];

/// The flags whose value is text the ledger stores, which may name a path without the broker reading it.
const TEXT_FLAGS: &[&str] = &["--body", "--box"];

/// The verbs whose positional words are text the ledger stores, so a slash inside one is not a path.
const TEXT_VERBS: &[&str] = &["note"];

/// An item reference such as `OWNER/REPO#N`, the one word with a slash that is not a path.
fn is_item_ref(word: &str) -> bool {
    word.split_once('#').is_some_and(|(repo, number)| repo.split_once('/').is_some_and(|(owner, name)| name_ok(owner) && name_ok(name)) && !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Each word is a declared flag, the value after one, or a positional; a flag with `=` or undeclared is refused, a flag value is refused when it is a path unless its flag stores text, and a positional of a verb that stores text is refused only when it starts like a path or a flag or climbs with `..`.
fn word_shapes(verb: &str, words: &[String]) -> Result<(), (u16, String)> {
    let mut after_flag: Option<bool> = None;
    for word in words {
        if word.starts_with("--") {
            after_flag = Some(flag_shape(word)?);
            continue;
        }
        match after_flag.take() {
            Some(true) => {}
            Some(false) => value_shape(word)?,
            None if TEXT_VERBS.contains(&verb) => text_shape(word)?,
            None => value_shape(word)?,
        }
    }
    Ok(())
}

/// Whether the flag stores text, its value then exempt from the path rule.
fn flag_shape(word: &str) -> Result<bool, (u16, String)> {
    if word.contains('=') || !FLAGS.contains(&word) {
        return Err((403, format!("{word:?} is not a flag a threads verb declares")));
    }
    Ok(TEXT_FLAGS.contains(&word))
}

fn value_shape(word: &str) -> Result<(), (u16, String)> {
    let path_like = word.starts_with(['/', '.', '~', '@', '-']) || word.contains(['/', '\\']) || word.contains("..");
    if is_number(word) || is_item_ref(word) || !path_like {
        return Ok(());
    }
    Err(refusal(word))
}

fn text_shape(word: &str) -> Result<(), (u16, String)> {
    let path_like = word.starts_with(['/', '.', '~', '@', '-']) || word.contains("..");
    if is_number(word) || is_item_ref(word) || !path_like {
        return Ok(());
    }
    Err(refusal(word))
}

fn refusal(word: &str) -> (u16, String) {
    if word.starts_with('-') {
        (403, format!("{word:?} is not a flag a threads verb declares"))
    } else {
        (403, format!("{word:?} names a path; threads requests carry values, not files"))
    }
}

fn is_number(word: &str) -> bool {
    word.strip_prefix('-').unwrap_or(word).bytes().all(|byte| byte.is_ascii_digit()) && !word.is_empty() && word != "-"
}

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
        word_shapes(verb, &words)?;
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
            .process_group(0)
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
                if let Some(group) = i32::try_from(pid).ok().and_then(rustix::process::Pid::from_raw) {
                    let _ = rustix::process::kill_process_group(group, rustix::process::Signal::Kill);
                }
                Err((504, "the ledger forward did not finish in time".to_owned()))
            }
        }
    }
}
