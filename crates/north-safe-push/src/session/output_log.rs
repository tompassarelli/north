use std::fs::File;
use std::os::fd::AsFd;
use std::path::Path;

use north_landing::landing::step::{Refused, Step};

pub struct Claims {
    pub list: String,
    _files: Vec<File>,
}

pub fn claim_output_log() -> Step<Claims> {
    let claimed_before = std::env::var("SAFE_PUSH_OUTPUT_CLAIMED").unwrap_or_default();
    let mut claims: Vec<(String, File)> = Vec::new();
    for fd in [1, 2] {
        let Ok(target) = std::fs::read_link(format!("/proc/self/fd/{fd}")) else { continue };
        let target = target.to_string_lossy().into_owned();
        let known = format!(" {} {claimed_before} ", claims.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>().join(" "));
        if !Path::new(&target).is_file() || known.contains(&format!(" {target} ")) {
            continue;
        }
        let Ok(file) = File::open(&target) else { return Err(Refused(1)) };
        if rustix::fs::flock(file.as_fd(), rustix::fs::FlockOperation::NonBlockingLockExclusive).is_err() {
            let cwd = std::env::current_dir().unwrap_or_default();
            eprintln!(
                "safe-push: refusing in {}: {target} is already the log of another running safe-push; give each landing its own log file. NOT pushed.",
                cwd.display()
            );
            return Err(Refused(1));
        }
        claims.push((target, file));
    }
    let added: String = claims.iter().map(|(path, _)| format!(" {path}")).collect();
    Ok(Claims { list: format!("{claimed_before}{added}"), _files: claims.into_iter().map(|(_, file)| file).collect() })
}
