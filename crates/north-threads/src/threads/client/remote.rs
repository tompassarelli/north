use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Seek, Write};
use std::process::Stdio;

use super::process::{Input, code_of, execute, execute_capturing_stderr, python_json};
use super::{Client, FORWARDED, shell_quote};
use crate::error::{NorthError, NorthResult};
use crate::foundation::exit::TIMED_OUT_STATUS;
use crate::platform::ssh_retry::{Link, Reply, Rerun, retried};

const LEDGER_IDENTITY_ENV: &str = "NORTH_LEDGER_IDENTITY";
const LEDGER_DESTINATION_ENV: &str = "NORTH_LEDGER_DESTINATION";

impl Client {
    pub(super) fn ledger_remote(&self) -> NorthResult<Option<String>> {
        if self.var("THREADS_DB").is_some() {
            return Ok(None);
        }
        if self.var(LEDGER_IDENTITY_ENV).is_some() {
            let dest = self.var(LEDGER_DESTINATION_ENV).ok_or_else(|| NorthError::Refused(format!("{LEDGER_DESTINATION_ENV} is unset; a ledger identity needs the destination as user@host")))?;
            return match dest.split_once('@') {
                Some((user, host)) if !user.is_empty() && !host.is_empty() && !dest.contains(char::is_whitespace) => Ok(Some(dest.to_owned())),
                _ => Err(NorthError::Refused(format!("{LEDGER_DESTINATION_ENV} must be user@host, not an alias"))),
            };
        }
        if !self.remote.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read_to_string(&self.remote)?.trim().to_owned()))
    }

    pub(super) fn no_native_reason(&self) -> Option<&'static str> {
        if self.var("THREADS_NO_NATIVE").is_some() {
            return Some("THREADS_NO_NATIVE is set (agy)");
        }
        self.var("CLAUDE_CODE_REMOTE").is_some().then_some("CLAUDE_CODE_REMOTE is set (cloud worker)")
    }

    pub(super) fn ssh(&self, dest: &str, args: &[String], input: Input, quiet: bool, err: &mut dyn Write) -> NorthResult<i32> {
        let command = self.remote_command(args);
        let argv = self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", dest, &command]);
        let stdout = if quiet { Stdio::null() } else { Stdio::inherit() };
        let wait = if args.first().map(String::as_str) == Some("job-run") { self.record_wait } else { self.ssh_wait };
        match execute(&argv, input, stdout, (wait, &self.await_child))? {
            Some(status) => Ok(code_of(status)),
            None => {
                writeln!(err, "NorthThreads: {dest} timed out; remote outcome unknown")?;
                Ok(TIMED_OUT_STATUS)
            }
        }
    }

    pub(super) fn ssh_capturing(&self, dest: &str, args: &[String], input: Input, quiet: bool, err: &mut dyn Write) -> NorthResult<(i32, String)> {
        let command = self.remote_command(args);
        let argv = self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", dest, &command]);
        let stdout = if quiet { Stdio::null() } else { Stdio::inherit() };
        let wait = if args.first().map(String::as_str) == Some("job-run") { self.record_wait } else { self.ssh_wait };
        let (status, stderr) = execute_capturing_stderr(&argv, input, stdout, (wait, &self.await_child))?;
        err.write_all(stderr.as_bytes())?;
        match status {
            Some(status) => Ok((code_of(status), stderr)),
            None => {
                writeln!(err, "NorthThreads: {dest} timed out; remote outcome unknown")?;
                Ok((TIMED_OUT_STATUS, stderr))
            }
        }
    }

    pub(super) fn remote_command(&self, args: &[String]) -> String {
        let mut remote = Vec::new();
        if let Some(actor) = self.var("THREADS_ACTOR") {
            remote.push(format!("THREADS_ACTOR={actor}"));
        }
        if self.no_native_reason().is_some() {
            remote.push("THREADS_NO_NATIVE=1".to_owned());
        }
        let first = args.first().map(String::as_str);
        if matches!(first, Some("unowned" | "claim" | "list")) && let Some(owners) = self.fleet.live_owners() {
            remote.push(format!("THREADS_ALIVE_OWNERS={}", python_json(&owners.into_iter().collect::<Vec<_>>())));
        }
        if first == Some("queue") && args.get(1).map(String::as_str) == Some("follow") {
            remote.push(format!("{FORWARDED}=1"));
        }
        if !remote.is_empty() {
            remote.insert(0, "env".to_owned());
        }
        remote.extend(["north".to_owned(), "threads".to_owned()]);
        remote.extend(args.iter().cloned());
        remote.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" ")
    }

    pub(super) fn forward(&self, words: &[&str], input: &[u8], err: &mut dyn Write) -> NorthResult<Option<i32>> {
        let Some(dest) = self.ledger_remote()? else { return Ok(None) };
        let command = words.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" ");
        let argv = self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", &dest, &command]);
        match execute(&argv, Input::Bytes(input.to_vec()), Stdio::inherit(), (self.ssh_wait, &self.await_child))? {
            Some(status) => Ok(Some(code_of(status))),
            None => {
                writeln!(err, "NorthThreads: {dest} timed out; remote outcome unknown")?;
                Ok(Some(TIMED_OUT_STATUS))
            }
        }
    }

    pub(super) fn ask(&self, words: &[&str], input: &[u8]) -> NorthResult<Option<Vec<u8>>> {
        let Some(dest) = self.ledger_remote()? else { return Ok(None) };
        let command = words.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" ");
        retried(&dest, Rerun::Anywhere, |link| self.ask_once(&dest, &command, input, link))
    }

    fn ask_once(&self, dest: &str, command: &str, input: &[u8], link: Link) -> Reply<NorthResult<Option<Vec<u8>>>> {
        let mut options = link.options().to_vec();
        options.extend(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", dest, command]);
        let argv = self.ssh_argv(&options);
        let answered = (|| {
            let mut output = tempfile::tempfile()?;
            let status = execute(&argv, Input::Bytes(input.to_vec()), Stdio::from(output.try_clone()?), (self.ssh_wait, &self.await_child))?;
            let mut answer = Vec::new();
            if status.is_some_and(|status| status.success()) {
                output.seek(io::SeekFrom::Start(0))?;
                output.read_to_end(&mut answer)?;
            }
            NorthResult::Ok((status, answer))
        })();
        match answered {
            Err(error) => Reply { value: Err(error), exit: None, said: String::new() },
            Ok((Some(status), answer)) if status.success() => Reply { value: Ok(Some(answer)), exit: Some(0), said: String::new() },
            Ok((Some(status), _)) => Reply { value: Err(NorthError::Refused(format!("{dest} answered exit {}", code_of(status)))), exit: Some(code_of(status)), said: String::new() },
            Ok((None, _)) => Reply { value: Err(NorthError::Refused(format!("{dest} timed out; remote outcome unknown"))), exit: None, said: String::new() },
        }
    }

    pub(super) fn ssh_argv(&self, arguments: &[&str]) -> Vec<OsString> {
        let mut options = Vec::new();
        if let Some(identity) = self.env.get(LEDGER_IDENTITY_ENV).filter(|identity| !identity.is_empty()) {
            options.extend(["-o", crate::platform::ssh::OWN_CONNECTION, "-F", "/dev/null", "-i", identity.as_str(), "-o", "IdentitiesOnly=yes"]);
        }
        options.extend_from_slice(arguments);
        crate::platform::ssh::argv(&self.ssh, &self.env, &options)
    }
}
