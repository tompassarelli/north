use super::{BESIDE_MISSING, RUNNING, SLOTS_BUSY, WRONG_TREE};

/// Copies the remote's stdout through to `out` without the status line the remote script prints after the command it ran, calls `running` once the remote holds its slot and runs, and returns that status.
pub(super) fn relay(remote: impl std::io::BufRead, out: &mut impl std::io::Write, marker: &str, mut running: impl FnMut()) -> Option<u8> {
    let tag = format!("{marker} ");
    let mut ran = None;
    for line in remote.split(b'\n') {
        let Ok(line) = line else { break };
        if line.starts_with(RUNNING.as_bytes()) {
            running();
        }
        let reported = String::from_utf8_lossy(&line).rfind(&tag).and_then(|at| std::str::from_utf8(&line[at + tag.len()..]).ok()?.parse::<u8>().ok().map(|code| (at, code)));
        let shown = match reported {
            Some((at, code)) => {
                ran = Some(code);
                &line[..at]
            }
            None => &line[..],
        };
        if reported.is_none() || !shown.is_empty() {
            let _ = out.write_all(shown).and_then(|()| out.write_all(b"\n")).and_then(|()| out.flush());
        }
    }
    ran
}

/// The run's exit code: the status the remote script reported for the command it ran, else the script's own refusal before running it; a run whose remote never reported its command's exit (a dropped connection, ssh exit 255, an exit before the report) reached no verdict, never the command's failure.
pub(super) fn verdict(exited: Option<u8>, ran: Option<u8>) -> Result<u8, String> {
    match (exited, ran) {
        (_, Some(code)) => Ok(code),
        (Some(code @ (WRONG_TREE | BESIDE_MISSING | SLOTS_BUSY)), None) => Ok(code),
        (exited, None) => Err(format!("the connection ended ({}) before the remote host reported the command's exit, so the run reached no verdict", exited.map_or_else(|| "ssh was killed by a signal".to_owned(), |code| format!("ssh exit {code}")))),
    }
}
