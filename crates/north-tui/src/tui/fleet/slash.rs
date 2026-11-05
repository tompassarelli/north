pub const COMMANDS: &[(&str, &str)] = &[
    ("/model", "choose the model for next turns"),
    ("/effort", "choose reasoning effort"),
    ("/goal", "show, set, or clear the standing goal"),
    ("/compact", "compact context"),
    ("/rename", "rename this session"),
    ("/fork", "start a sibling from this transcript"),
    ("/config", "open the Context Switchboard"),
    ("/help", "show keys and commands"),
];

pub fn matching(filter: &str) -> Vec<(&'static str, &'static str)> {
    COMMANDS.iter().copied().filter(|(name, _)| name.starts_with(filter)).collect()
}
