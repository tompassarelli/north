const CLIPPY: &str = "cargo clippy";

pub fn listed(commands: &[String], listed: impl Fn(&str) -> bool) -> Option<String> {
    commands.iter().find(|command| command.contains(CLIPPY) && listed(command)).cloned()
}
