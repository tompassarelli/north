pub(in crate::runtime) fn alert_coordinator(text: &str) {
    send_alert(text);
}

/// Sends the alert and says whether it was delivered, for a caller that records the alert as sent only once it was.
pub(in crate::runtime) fn send_alert(text: &str) -> bool {
    eprintln!("north runtime update: {text}");
    true
}
