pub const COMMANDS: &[(&str, &str)] = &[
    ("/model", "choose the model for next turns"),
    ("/effort", "choose reasoning effort"),
    ("/help", "show keys and commands"),
];

pub fn matching(filter: &str) -> Vec<(&'static str, &'static str)> {
    COMMANDS.iter().copied().filter(|(name, _)| name.starts_with(filter)).collect()
}
