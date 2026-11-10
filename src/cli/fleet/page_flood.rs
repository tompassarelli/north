use std::process::{Command, Stdio};

use crate::fleet::flood::CLASS;
use crate::fleet::sender::Sender;

/// Records the page flood `line` told the proxy of as one `page-flood` incident per sender and minute.
pub(crate) fn record(from: &Sender, line: &str) {
    let minute = crate::fleet::runtime::now_ms() / 60_000;
    let recorded = Command::new(crate::platform::north_binary::north_binary())
        .args(["ledger", "incident", "record", "--class", CLASS, "--noticed-by", "north-fleet"])
        .args(["--key", &format!("{CLASS}:{from}@{minute}"), "--subject", &from.to_string(), "--detail", line])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status();
    if !recorded.is_ok_and(|status| status.success()) {
        eprintln!("north fleet: the {CLASS} incident for {from} was not recorded");
    }
}
