use std::collections::BTreeMap;
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::ops::remote_serve::{self, Grants, Launch, LedgerJobs, Plan, audit, log};
use crate::threads::{client, outside::FleetView};

const USAGE: &str = "Usage: north remote-serve --peer HOST [--repos REPO,...] [--log FILE] | north remote-serve audit --peer HOST [--log FILE] [--keys FILE]";
const UNLAUNCHED: i32 = 127;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Options {
    pub audit: bool,
    pub peer: String,
    pub repos: Vec<String>,
    pub log: Option<PathBuf>,
    pub keys: Option<PathBuf>,
}

pub(super) struct Host {
    pub env: BTreeMap<String, String>,
    pub exe: PathBuf,
    pub fleet: Arc<dyn FleetView>,
}

pub(super) fn run(arguments: &[String]) -> NorthResult<u8> {
    let options = options(arguments)?;
    let env = client::ambient_env();
    if options.audit {
        print!("{}", audit_text(&options, &env, Timestamp::now().unix_days())?);
        return Ok(0);
    }
    let host = Host { exe: std::env::current_exe()?, fleet: Arc::new(super::fleet::Registry), env };
    let command = host.env.get("SSH_ORIGINAL_COMMAND").cloned();
    Ok(exit_byte(serve(&options, command.as_deref(), &host, &mut io::stderr())))
}

pub(super) fn options(arguments: &[String]) -> NorthResult<Options> {
    let usage = || NorthError::Usage(USAGE.into());
    let (audit, mut rest) = match arguments.split_first() {
        Some((first, rest)) if first == "audit" => (true, rest.iter()),
        _ => (false, arguments.iter()),
    };
    let mut options = Options { audit, ..Options::default() };
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(usage)?;
        match flag.as_str() {
            "--peer" => options.peer = value.clone(),
            "--repos" if !audit => options.repos = value.split(',').filter(|repo| !repo.is_empty()).map(str::to_owned).collect(),
            "--log" => options.log = Some(value.into()),
            "--keys" if audit => options.keys = Some(value.into()),
            _ => return Err(usage()),
        }
    }
    if !remote_serve::peer_name(&options.peer) {
        return Err(NorthError::Usage(format!("--peer {:?} is not a host name\n{USAGE}", options.peer)));
    }
    Ok(options)
}

fn home(env: &BTreeMap<String, String>) -> NorthResult<&Path> {
    env.get("HOME").map(Path::new).ok_or_else(|| NorthError::Usage("no home directory".into()))
}

fn log_path(options: &Options, env: &BTreeMap<String, String>) -> NorthResult<PathBuf> {
    options.log.clone().map_or_else(|| Ok(log::path(home(env)?, &options.peer)), Ok)
}

pub(super) fn serve(options: &Options, command: Option<&str>, host: &Host, err: &mut dyn Write) -> i32 {
    let opened = log_path(options, &host.env).map_err(|error| error.to_string()).and_then(|path| log::Log::open(&path).map_err(|error| format!("{}: {error}", path.display())));
    let mut log = match opened {
        Ok(log) => log,
        Err(why) => {
            let _ = writeln!(err, "remote-serve: refused for {}: the call log cannot be opened ({why})", options.peer);
            return log::REFUSED_EXIT;
        }
    };
    let grants = Grants { peer: options.peer.clone(), repos: options.repos.clone() };
    let jobs = LedgerJobs(remote_serve::ledger(&host.env).unwrap_or_default());
    match remote_serve::serve(command, &grants, &jobs, host, &mut log, err) {
        Ok(exit) => exit,
        Err(error) => {
            let _ = writeln!(err, "remote-serve: the call ran but its log row was not written: {error}");
            1
        }
    }
}

pub(super) fn audit_text(options: &Options, env: &BTreeMap<String, String>, today: i64) -> NorthResult<String> {
    let read = log::read(&log_path(options, env)?)?;
    let keys = match &options.keys {
        Some(path) => Some(path.clone()),
        None => home(env).ok().map(|home| home.join(".ssh/authorized_keys")),
    };
    let keys = match keys.map(std::fs::read_to_string) {
        Some(Ok(text)) => Some(text),
        Some(Err(error)) if error.kind() == io::ErrorKind::NotFound => None,
        Some(Err(error)) => return Err(error.into()),
        None => None,
    };
    Ok(audit::audit(&options.peer, &read, keys.as_deref(), today).to_string())
}

impl Launch for Host {
    fn launch(&self, plan: &Plan) -> i32 {
        let mut env = self.env.clone();
        env.extend(plan.env.clone());
        let words: Vec<&str> = plan.argv.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["threads", ..] | ["north", "threads", ..] => {
                let skip = if words[0] == "north" { 2 } else { 1 };
                i32::from(client::run_in(&plan.argv[skip..], env, self.fleet.clone()))
            }
            ["north", ..] => spawn(self.exe.as_os_str(), &plan.argv[1..], &env),
            [program, ..] => spawn(program.as_ref(), &plan.argv[1..], &env),
            [] => UNLAUNCHED,
        }
    }
}

fn spawn(program: &std::ffi::OsStr, arguments: &[String], env: &BTreeMap<String, String>) -> i32 {
    match Command::new(program).args(arguments).env_clear().envs(env).status() {
        Ok(status) => status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
        Err(error) => {
            eprintln!("remote-serve: {} did not start: {error}", program.to_string_lossy());
            UNLAUNCHED
        }
    }
}

fn exit_byte(code: i32) -> u8 {
    u8::try_from(code).unwrap_or(1)
}
