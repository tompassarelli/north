use super::{BESIDE_MISSING, RUNNING, SLOTS_BUSY, WRONG_TREE};

/// What the remote reported on its own lines: the status of the command it ran, and the tree it proved that run was computed on, on a line opened by the run's secret.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Relayed {
    pub(super) ran: Option<u8>,
    pub(super) tree: Option<String>,
}

/// Copies the remote's stdout through to `out` without the status line the remote script prints after the command it ran or the line it opens with `secret`, calls `running` once the remote holds its slot and runs, and returns what those lines reported.
pub(super) fn relay(remote: impl std::io::BufRead, out: &mut impl std::io::Write, marker: &str, secret: &str, mut running: impl FnMut()) -> Relayed {
    let (tag, vouched) = (format!("{marker} "), format!("{secret} "));
    let mut relayed = Relayed::default();
    for line in remote.split(b'\n') {
        let Ok(line) = line else { break };
        if line.starts_with(RUNNING.as_bytes()) {
            running();
        }
        let text = String::from_utf8_lossy(&line);
        let after = |tag: &str| text.rfind(tag).and_then(|at| Some((at, std::str::from_utf8(&line[at + tag.len()..]).ok()?)));
        let reported = after(&tag).and_then(|(at, rest)| Some((at, rest.parse::<u8>().ok()?)));
        let proved = after(&vouched).filter(|_| !secret.is_empty()).map(|(at, rest)| (at, super::proof::tree(rest)));
        let at = match (reported, proved) {
            (Some((at, code)), _) => {
                relayed.ran = Some(code);
                Some(at)
            }
            (None, Some((at, tree))) => {
                relayed.tree = tree;
                Some(at)
            }
            (None, None) => None,
        };
        let shown = &line[..at.unwrap_or(line.len())];
        if at.is_none() || !shown.is_empty() {
            let _ = out.write_all(shown).and_then(|()| out.write_all(b"\n")).and_then(|()| out.flush());
        }
    }
    relayed
}

/// The run's exit code: the status the remote script reported for the command it ran, else the script's own refusal before running it; a run whose remote never reported its command's exit (a dropped connection, ssh exit 255, an exit before the report) reached no verdict, never the command's failure.
pub(super) fn verdict(exited: Option<u8>, ran: Option<u8>) -> Result<u8, String> {
    match (exited, ran) {
        (_, Some(code)) => Ok(code),
        (Some(code @ (WRONG_TREE | BESIDE_MISSING | SLOTS_BUSY)), None) => Ok(code),
        (exited, None) => Err(format!("the connection ended ({}) before the remote host reported the command's exit, so the run reached no verdict", exited.map_or_else(|| "ssh was killed by a signal".to_owned(), |code| format!("ssh exit {code}")))),
    }
}
