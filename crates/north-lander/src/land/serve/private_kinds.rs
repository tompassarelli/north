use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use north_base::foundation::exit::NO_VERDICT;

use crate::land::after::route::{REMOTE_NORTH, SSH_OPTIONS, quote, session_login};
use crate::land::enqueue::default_branch;
use crate::land::queue::Ticket;
use crate::land::repositories;
use crate::land::scans::outgoing::JUDGING;
use crate::land::scans::outgoing::denylist::{Denylist, DenylistVerdict, Stand};

pub(super) fn verdict(repo: &Path, ticket: &Ticket, base: &str, wait: Option<Duration>) -> DenylistVerdict {
    if !asked_elsewhere(&ticket.host) {
        return Denylist::from_env().verdict_within(&ticket.root, &ticket.origin, (base, &ticket.head, &default_branch(&ticket.root)), wait);
    }
    let script = script(ticket, base, &default_branch(repo), wait);
    let login = match dialed(&ticket.host) {
        Ok(login) => login,
        Err(why) => return unreachable(&ticket.host, &why),
    };
    read(&ticket.host, Command::new("ssh").args(SSH_OPTIONS).args(["-T", &login, "--", &script]).stdin(Stdio::null()).output())
}

fn dialed(host: &str) -> Result<String, String> {
    dialed_through(host, session_login)
}

fn dialed_through(host: &str, login: impl Fn(&str) -> Result<String, String>) -> Result<String, String> {
    let named = host.split('.').next().unwrap_or(host);
    let session = login(named)?;
    Ok(if session == named { host.to_owned() } else { session })
}

fn asked_elsewhere(host: &str) -> bool {
    !host.is_empty() && host.split('.').next() != repositories::local_host().split('.').next()
}

fn script(ticket: &Ticket, base: &str, reference: &str, wait: Option<Duration>) -> String {
    let wait = wait.map(|wait| format!(" {}", wait.as_millis())).unwrap_or_default();
    format!(
        "cd {} 2>/dev/null || exit {NO_VERDICT}; {REMOTE_NORTH} exec \"$n\" land scan private-kinds {} {} {}{wait}",
        quote(&ticket.root.to_string_lossy()),
        quote(base),
        quote(&ticket.head),
        quote(reference)
    )
}

pub(super) fn read(host: &str, ran: io::Result<Output>) -> DenylistVerdict {
    let output = match ran {
        Ok(output) => output,
        Err(error) => return unreachable(host, &error.to_string()),
    };
    let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stand = match output.status.code() {
        Some(0) => Stand::Pass,
        Some(1) => Stand::Refuse,
        Some(code) if code == i32::from(JUDGING) => Stand::Judging,
        Some(255) | None => return unreachable(host, &stderr),
        Some(code) => {
            lines.push(format!("north land: {host} has no private-kind verdict on this lane (exit {code}): {stderr}"));
            Stand::NoVerdict
        }
    };
    DenylistVerdict { lines, stand }
}

fn unreachable(host: &str, why: &str) -> DenylistVerdict {
    DenylistVerdict { lines: vec![format!("north land: cannot reach {host} to read the private-kind verdict its hand-in asked for ({why}); the lane waits for it")], stand: Stand::Judging }
}

#[cfg(test)]
#[path = "private_kinds_tests.rs"]
mod tests;
