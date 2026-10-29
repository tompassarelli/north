use super::bounded;
use super::layout::{self, PROGRAMS};
use super::remote::limit;
use super::store::references;
use crate::ops::land_offload;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const COPY: Duration = Duration::from_secs(10 * 60);
const REALISE: Duration = Duration::from_secs(10 * 60);

pub(super) fn pull(target: &str, runtime: &Path) -> bool {
    let short = layout::short(target);
    for host in land_offload::other_destinations() {
        match fetch(&host, target, runtime) {
            Ok(()) => {
                println!("north runtime: pulled {short} built on {host} and realised its store closure");
                return true;
            }
            Err(why) => {
                eprintln!("north runtime: {host} has no usable {short}: {why}");
                for program in PROGRAMS {
                    let _ = fs::remove_file(runtime.join(program));
                }
            }
        }
    }
    false
}

fn fetch(host: &str, target: &str, runtime: &Path) -> Result<(), String> {
    fs::create_dir_all(runtime).map_err(|error| error.to_string())?;
    let mut copy = Command::new("rsync");
    copy.args(["-a", "--timeout=60", "-e", "ssh -o BatchMode=yes -o ConnectTimeout=20"])
        .args(PROGRAMS.map(|program| format!("{host}:.local/state/north/runtimes/{target}/{program}")))
        .arg(runtime);
    step("rsync", &mut copy, limit(COPY))?;
    if !layout::has_programs(runtime) {
        return Err("the copy left the programs incomplete".to_owned());
    }
    let references = references(runtime).map_err(|failure| failure.message)?;
    let mut realise = Command::new("nix-store");
    realise.args(["--realise", "--quiet"]).args(&references).stdout(std::process::Stdio::null());
    step("nix-store --realise", &mut realise, limit(REALISE))
}

fn step(name: &str, command: &mut Command, limit: Duration) -> Result<(), String> {
    match bounded::status(command, limit) {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{name} failed: {status}")),
        Err(error) => Err(format!("{name} did not run to completion: {error}")),
    }
}
