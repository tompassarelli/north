pub fn listed(commands: &[String], listed: impl Fn(&str) -> bool) -> Option<String> {
    commands.iter().find(|command| north_build_system::lints(command) && listed(command)).cloned()
}
