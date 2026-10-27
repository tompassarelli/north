//! The facets an issue declares in its body (`Category: tooling`), the defaults dispatch assumes
//! for the ones it leaves out, and whether the box needs a native lane.
use super::issues::Issue;

pub(crate) const FACETS: [(&str, &str); 5] = [("Category", "feature"), ("Spec", "exact"), ("Scope", "module"), ("Surface", "ts"), ("Verify", "local-test")];
const NATIVE_LABELS: [&str; 3] = ["needs:native-pair", "needs:native-single", "evidence:native"];

pub(crate) fn declared<'a>(issue: &'a Issue, key: &str) -> Option<&'a str> {
    declared_in(&issue.body, key)
}

pub(crate) fn declared_in<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.trim().trim_start_matches(['-', '*', ' ']).split_once(':')?;
        let value = value.trim().trim_matches('`');
        (name.trim().trim_matches('*').eq_ignore_ascii_case(key) && !value.is_empty()).then_some(value)
    })
}

/// A box needs a native lane when a label says so or an unticked Done-when box is tagged `(native)`.
pub(crate) fn native(issue: &Issue) -> bool {
    issue.labelled(&NATIVE_LABELS).is_some()
        || issue.body.lines().any(|line| {
            let line = line.trim_start();
            line.strip_prefix("- [ ]").or_else(|| line.strip_prefix("* [ ]")).is_some_and(|rest| rest.trim_start().starts_with("(native"))
        })
}

pub(crate) fn default(key: &str) -> &'static str {
    FACETS.iter().find(|(name, _)| *name == key).map_or("", |(_, default)| default)
}

/// A facet the issue declares, else `native` for Verify on a native box, else the default.
pub(crate) fn facet(issue: &Issue, key: &str) -> String {
    let default = default(key);
    match (key, declared(issue, key)) {
        (_, Some(value)) => value.to_owned(),
        ("Verify", None) if native(issue) => "native".into(),
        _ => default.into(),
    }
}
