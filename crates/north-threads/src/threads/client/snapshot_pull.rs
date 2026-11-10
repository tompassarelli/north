use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use super::Client;
use super::process::detach;
use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::threads::snapshot;

impl Client {
    pub(super) fn refresh_after_answer(&self, dest: &str, argv: &[String], code: i32) -> NorthResult<()> {
        let first = argv.first().map(String::as_str);
        let has = |flag: &str| argv.iter().any(|arg| arg == flag);
        let calibrate_write = first == Some("calibrate") && has("--write");
        if code == 0 && calibrate_write && has("--goal-row") {
            return Ok(());
        }
        if code == 0 && (first == Some("record-priors") || calibrate_write) {
            return self.refresh_snapshot(dest, Duration::ZERO);
        }
        self.refresh_snapshot_later(dest, snapshot::SNAPSHOT_AGE);
        Ok(())
    }

    pub(super) fn stale_line(&self, dest: &str) -> NorthResult<String> {
        let modified = fs::metadata(&self.snapshot)?.modified()?;
        Ok(format!("STALE: ledger snapshot from {}; {dest} unreachable", Timestamp::from(modified).iso()))
    }

    pub(super) fn refresh_snapshot(&self, dest: &str, within: Duration) -> NorthResult<()> {
        let _ = snapshot::refresh(&self.pull_argv(dest), &self.snapshot, within, snapshot::PULL_TIMEOUT);
        Ok(())
    }

    pub(super) fn refresh_snapshot_later(&self, dest: &str, within: Duration) {
        let Some(program) = &self.background else {
            let _ = self.refresh_snapshot(dest, within);
            return;
        };
        if !snapshot::take_turn(&self.snapshot, within).unwrap_or(false) {
            return;
        }
        detach(program, &["snapshot-pull".as_ref(), "--force".as_ref(), "--snapshot".as_ref(), self.snapshot.as_os_str(), dest.as_ref()]);
    }

    pub(super) fn pull_argv(&self, dest: &str) -> Vec<OsString> {
        self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", "-o", crate::platform::ssh::OWN_CONNECTION, dest, snapshot::REMOTE_COMMAND])
    }

    pub(super) fn snapshot_verb(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<i32> {
        if argv[0] == "snapshot-pages" {
            let database = self.var("THREADS_DB").map_or_else(|| self.home.join(".local/state/threads/threads.db"), PathBuf::from);
            return match snapshot::serve(&database, self.no_native_reason().is_some(), stdin, &mut io::stdout().lock()) {
                Ok(()) => Ok(0),
                Err(error) => {
                    writeln!(err, "NorthThreads: snapshot pages failed: {error}")?;
                    Ok(1)
                }
            };
        }
        let (mut force, mut path, mut dest) = (false, self.snapshot.clone(), None);
        let mut rest = argv[1..].iter();
        while let Some(arg) = rest.next() {
            match arg.as_str() {
                "--force" => force = true,
                "--snapshot" => path = rest.next().map(PathBuf::from).ok_or_else(|| NorthError::Usage("--snapshot needs a path".into()))?,
                _ if dest.is_none() && !arg.starts_with('-') => dest = Some(arg.clone()),
                _ => return Err(NorthError::Usage("usage: north threads snapshot-pull [--force] [--snapshot PATH] DEST".into())),
            }
        }
        let dest = dest.ok_or_else(|| NorthError::Usage("usage: north threads snapshot-pull [--force] [--snapshot PATH] DEST".into()))?;
        match snapshot::refresh(&self.pull_argv(&dest), &path, if force { Duration::ZERO } else { snapshot::SNAPSHOT_AGE }, snapshot::PULL_TIMEOUT) {
            Ok(_) => Ok(0),
            Err(error) => {
                writeln!(err, "NorthThreads: snapshot pull from {dest} failed: {error}")?;
                Ok(1)
            }
        }
    }
}
