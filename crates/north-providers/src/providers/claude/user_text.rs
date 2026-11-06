use crate::fleet::model::EntryKind;

pub fn entry(text: &str, replay: bool, synthetic: bool) -> (EntryKind, String) {
    let sender = if replay { EntryKind::User } else { EntryKind::Notice };
    let head = text.trim_start();
    if head.starts_with("<command-name>") {
        let command = format!("{} {}", tagged(text, "command-name").trim(), tagged(text, "command-args").trim());
        return (sender, command.trim_end().to_owned());
    }
    if head.starts_with("<local-command-stdout>") {
        return (EntryKind::Notice, tagged(text, "local-command-stdout").trim().to_owned());
    }
    if head.starts_with("<local-command-stderr>") {
        return (EntryKind::Error, tagged(text, "local-command-stderr").trim().to_owned());
    }
    if synthetic {
        return (EntryKind::Notice, head.lines().next().unwrap_or_default().to_owned());
    }
    (sender, text.to_owned())
}

fn tagged<'t>(text: &'t str, tag: &str) -> &'t str {
    let inner = text.split_once(&format!("<{tag}>")).map_or("", |(_, rest)| rest);
    inner.split_once(&format!("</{tag}>")).map_or(inner, |(value, _)| value)
}
