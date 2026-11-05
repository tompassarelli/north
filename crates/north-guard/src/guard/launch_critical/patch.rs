use super::shell::{leads_with, leads_with_rescue, resolve};
use super::{PATCH_BEGIN, PATCH_ENVELOPE, PATCH_HEADER, PATCH_MOVE, Policy, SEGMENT_SPLIT};
use crate::shell::{basename, strip_heredoc_bodies};
use regex::Regex;
use std::sync::LazyLock;

pub(super) fn excise_sanctioned(text: &str) -> String {
    let mut out = String::new();
    let mut index = 0;
    for m in SEGMENT_SPLIT.find_iter(text) {
        let segment = &text[index..m.start()];
        if !leads_with_rescue(segment) {
            out.push_str(segment);
        }
        out.push_str(m.as_str());
        index = m.end();
    }
    if !leads_with_rescue(&text[index..]) {
        out.push_str(&text[index..]);
    }
    out
}
pub fn patch_targets(envelope: &str) -> Vec<String> {
    PATCH_HEADER
        .captures_iter(envelope)
        .chain(PATCH_MOVE.captures_iter(envelope))
        .map(|m| m[1].into())
        .collect()
}
fn patch_removals(envelope: &str) -> Vec<String> {
    static BLOCK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\*\*\* (Add|Update|Delete) File:").unwrap());
    static DELETED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?m)^\*\*\* (?:Delete|Update) File:[\s\x1c-\x1f]+(.+?)[\s\x1c-\x1f]*$")
            .unwrap()
    });
    let matches: Vec<_> = BLOCK.captures_iter(envelope).collect();
    let mut removed = Vec::new();
    for (i, m) in matches.iter().enumerate() {
        let start = m.get(0).unwrap().start();
        let end = matches
            .get(i + 1)
            .map_or(envelope.len(), |n| n.get(0).unwrap().start());
        let block = &envelope[start..end];
        if (&m[1] == "Delete" || (&m[1] == "Update" && PATCH_MOVE.is_match(block)))
            && let Some(header) = DELETED.captures(block)
        {
            removed.push(header[1].into());
        }
    }
    removed
}
pub(super) fn find_envelope(raw: &serde_json::value::RawValue) -> Option<String> {
    if raw.get().starts_with('"') {
        let text: String = serde_json::from_str(raw.get()).ok()?;
        return text.contains(PATCH_BEGIN).then_some(text);
    }
    if !raw.get().starts_with('{') {
        return None;
    }
    struct Envelope;
    impl<'de> serde::de::Visitor<'de> for Envelope {
        type Value = Option<String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an object containing a patch envelope")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut entries: Vec<(String, Box<serde_json::value::RawValue>)> = Vec::new();
            while let Some((key, value)) =
                map.next_entry::<String, Box<serde_json::value::RawValue>>()?
            {
                if let Some((_, previous)) = entries.iter_mut().find(|(name, _)| name == &key) {
                    *previous = value;
                } else {
                    entries.push((key, value));
                }
            }
            Ok(entries
                .into_iter()
                .find_map(|(_, value)| find_envelope(&value)))
        }
    }
    serde::Deserializer::deserialize_map(
        &mut serde_json::Deserializer::from_str(raw.get()),
        Envelope,
    )
    .ok()
    .flatten()
}
fn invokes_apply_patch(text: &str) -> bool {
    SEGMENT_SPLIT
        .split(text)
        .any(|s| leads_with(s, "apply_patch"))
}
fn patch_fail_closed() -> String {
    "This apply_patch call is denied fail-closed: a patch whose targets cannot be read cannot be checked. Re-issue it with well-formed `*** Add/Update/Delete File:` headers. Deliberate bypass: `north config agents off launch-critical-worktree-guard`.".into()
}
impl Policy {
    fn patch_decision(&self, targets: &[String], cwd: &str, removals: &[String]) -> Option<String> {
        for target in removals {
            let path = resolve(&self.expanduser(target), cwd)?;
            if let Some(pin) = self.sidecar(&path) {
                return Some(format!(
                    "This apply_patch envelope would delete live pin consumer metadata at {path}. Retire the checkout and sidecar together after proving every named consumer moved:\n\n  north pin-retire --consumer-main CONSUMER/main -- {pin}"
                ));
            }
        }
        for target in targets {
            if let Some(path) = resolve(&self.expanduser(target), cwd)
                && let Some(hit) = self.protected(&path)
            {
                return Some(format!(
                    "This apply_patch envelope would write {path} inside {}. {}\n\n{}",
                    hit.noun(),
                    hit.why,
                    self.advice(&hit)
                ));
            }
        }
        None
    }
    pub(super) fn patch_tool(&self, input: &crate::platform::pyjson::Value, cwd: &str, envelope: Option<&str>) -> Option<String> {
        let explicit: Vec<_> = ["file_path", "path"]
            .iter()
            .filter_map(|key| input[key].as_str().map(str::to_owned))
            .collect();
        if let Some(reason) = self.patch_decision(&explicit, cwd, &[]) {
            return Some(reason);
        }
        let selected = input["workdir"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| input["cwd"].as_str());
        let patch_cwd = selected.unwrap_or(cwd);
        let Some(envelope) = envelope else {
            return Some(patch_fail_closed());
        };
        let targets = patch_targets(envelope);
        if targets.is_empty() {
            return Some(patch_fail_closed());
        }
        self.patch_decision(&targets, patch_cwd, &patch_removals(envelope))
    }
    pub(super) fn patch_command(&self, command: &crate::platform::pyjson::Value, cwd: &str, stripped: Option<&str>) -> Option<String> {
        let (invoked, text) = if let Some(text) = command.as_str() {
            (
                invokes_apply_patch(stripped.unwrap_or(&strip_heredoc_bodies(text))),
                text.to_owned(),
            )
        } else if let Some(argv) = command.as_array() {
            let argv: Vec<_> = argv.iter().map(crate::platform::pyjson::Value::as_str).collect::<Option<_>>()?;
            let mut invoked = argv.first().is_some_and(|s| basename(s) == "apply_patch");
            if !invoked
                && argv
                    .first()
                    .is_some_and(|s| ["bash", "sh", "zsh"].contains(&basename(s)))
            {
                for i in 1..argv.len().saturating_sub(1) {
                    let flag = argv[i];
                    if flag.starts_with('-') && flag[1..].contains('c') {
                        invoked = invokes_apply_patch(&strip_heredoc_bodies(argv[i + 1]));
                        break;
                    }
                }
            }
            (invoked, argv.join("\n"))
        } else {
            return None;
        };
        if !invoked {
            return None;
        }
        let envelopes: Vec<_> = PATCH_ENVELOPE.find_iter(&text).collect();
        if envelopes.is_empty() {
            return Some(patch_fail_closed());
        }
        for envelope in envelopes {
            let targets = patch_targets(envelope.as_str());
            if targets.is_empty() {
                return Some(patch_fail_closed());
            }
            if let Some(reason) =
                self.patch_decision(&targets, cwd, &patch_removals(envelope.as_str()))
            {
                return Some(reason);
            }
        }
        None
    }
}
