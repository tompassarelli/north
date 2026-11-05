use std::env;
use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::Job;
use super::records::{find, north, search_path};
use crate::foundation::clock::Timestamp;

pub(super) const LOG: &str = "JOB_RUN_LOG";

fn logs() -> PathBuf {
    let state = env::var_os("XDG_STATE_HOME").filter(|state| !state.is_empty()).map_or_else(|| PathBuf::from(env::var("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from);
    state.join("north/jobs")
}

pub(super) fn run(job: &Job, arguments: &[String], session: &str) -> u8 {
    let path = search_path(&job.record_path);
    let directory = logs();
    if let Err(error) = fs::create_dir_all(&directory) {
        eprintln!("job-run: {}: {error}", directory.display());
        return 1;
    }
    let stamp = Timestamp::now().iso().replace(['-', ':', 'Z'], "");
    let log = directory.join(format!("{}-{stamp}-{}.log", job.name.replace('/', "_"), std::process::id()));
    let Some(setsid) = find("setsid", &path) else {
        eprintln!("job-run: --notify needs setsid");
        return 1;
    };
    let scope = find("systemd-run", &path).filter(|systemd_run| {
        Command::new(systemd_run)
            .args(["--user", "--scope", "--quiet", "--collect", "true"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    });
    let output = File::create(&log).and_then(|output| Ok((output.try_clone()?, output)));
    let detached = output.and_then(|(errors, output)| {
        let mut command = Command::new(setsid);
        command.arg("-f");
        if let Some(scope) = scope {
            command.arg(scope).args(["--user", "--scope", "--quiet", "--collect", "--slice=agent.slice"]);
        }
        command
            .arg(north())
            .arg("job-run")
            .args(arguments)
            .env(LOG, &log)
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(errors)
            .status()
    });
    if !detached.is_ok_and(|status| status.success()) {
        eprintln!("job-run: could not detach {}; see {}", job.name, log.display());
        return 1;
    }
    println!("job-run: {} runs detached with its output in {}; its exit arrives as a fleet message to {session}, so end your turn.", job.name, log.display());
    0
}
