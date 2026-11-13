use std::collections::BTreeMap;

pub(super) fn origin(s: &str) -> bool {
    matches!(s, "tom" | "experiment" | "prior-art" | "incident")
}
pub(super) fn fields(value: &str) -> (&str, BTreeMap<&str, &str>, bool) {
    let parts: Vec<_> = value.split(';').map(str::trim).collect();
    let named = parts
        .iter()
        .skip(1)
        .filter_map(|p| {
            p.find(char::is_whitespace)
                .map(|i| (&p[..i], p[i..].trim_start()))
        })
        .collect();
    (parts[0], named, parts.last() == Some(&"soft"))
}
pub(super) fn records<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    let pattern = match key {
        "Provenance:" => re!(r"(?m)^Provenance:\s*(.+)$"),
        "Target-debt:" => re!(r"(?m)^Target-debt:\s*(.+)$"),
        "Target-paid:" => re!(r"(?m)^Target-paid:\s*(.+)$"),
        _ => unreachable!(),
    };
    pattern
        .captures_iter(text)
        .map(|m| m.get(1).unwrap().as_str())
        .collect()
}
pub(super) fn valid_record(value: &str) -> bool {
    let (claim, named, soft) = fields(value);
    !claim.is_empty()
        && ["pointer", "rationale", "revisit"]
            .iter()
            .all(|k| named.get(k).is_some_and(|v| !v.is_empty()))
        && named
            .get("rationale")
            .is_some_and(|r| *r == "not recorded" || re!(r"\A[^\s#]+#[^\s#]+\z").is_match(r))
        && (origin(named.get("origin").copied().unwrap_or(""))
            || soft && named.get("origin") == Some(&"none"))
}
