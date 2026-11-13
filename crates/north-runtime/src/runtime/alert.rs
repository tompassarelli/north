use std::process::Command;

pub(in crate::runtime) fn alert_proxy(text: &str) {
    send_alert(text);
}

/// Sends the alert and says whether it was delivered, for a caller that records the alert as sent only once it was.
pub(in crate::runtime) fn send_alert(text: &str) -> bool {
    let sent = Command::new("north")
        .args(["fleet", "send", "--from", "service:north-runtime-update", "proxy", text])
        .status()
        .is_ok_and(|status| status.success());
    if !sent {
        eprintln!("north runtime update: proxy alert delivery failed");
    }
    sent
}
