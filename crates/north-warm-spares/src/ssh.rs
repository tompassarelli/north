use std::time::Duration;

pub const SSH_WAIT: Duration = Duration::from_secs(20);

pub fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}
