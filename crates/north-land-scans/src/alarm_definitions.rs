use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use toml_edit::{DocumentMut, Item};

use super::{Range, git};

pub const USAGE: &str = "usage: north land scan alarm-definitions BASE..HEAD";

const REGISTRY: &str = "harness/alarms.toml";

static SENDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"service:(north-[a-z0-9-]+)").expect("sender pattern"));

/// The sources `text` defines, or each way a row fails to name its action and owner.
pub fn defined(text: &str) -> Result<BTreeSet<String>, Vec<String>> {
    let document = text.parse::<DocumentMut>().map_err(|error| vec![format!("{REGISTRY} does not parse: {error}")])?;
    let rows = document.get("alarm").and_then(Item::as_array_of_tables).into_iter().flatten();
    let mut sources = BTreeSet::new();
    let mut refusals = Vec::new();
    for (at, row) in rows.enumerate() {
        let field = |key: &str| row.get(key).and_then(Item::as_str).map(str::trim).filter(|text| !text.is_empty());
        let Some(source) = field("source") else {
            refusals.push(format!("{REGISTRY}: alarm {} names no `source`", at + 1));
            continue;
        };
        for key in ["action", "owner"] {
            if field(key).is_none() {
                refusals.push(format!("{REGISTRY}: the alarm of {source} names no `{key}`, so it asks nothing of anyone; name it or delete the alarm"));
            }
        }
        sources.insert(source.to_owned());
    }
    match refusals.is_empty() {
        true => Ok(sources),
        false => Err(refusals),
    }
}

/// The `service:UNIT` senders the lane's diff adds to code outside tests.
pub fn added_senders(patch: &str) -> BTreeSet<String> {
    let mut file = "";
    let mut senders = BTreeSet::new();
    for line in patch.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            file = path;
        } else if line.starts_with('+') && file.ends_with(".rs") && !file.contains("tests") && !file.contains("/fixtures/") {
            senders.extend(SENDER.captures_iter(line).map(|found| found[1].to_owned()));
        }
    }
    senders
}

pub fn refusals(directory: &Path, base: &str, head: &str) -> Vec<String> {
    let at = |path: &str| git::text(directory, &["show", &format!("{head}:{path}")]);
    let Some(text) = at(REGISTRY) else { return vec![format!("{REGISTRY} is missing: every alarm service needs a row naming its action and owner")] };
    let sources = match defined(&text) {
        Ok(sources) => sources,
        Err(refusals) => return refusals,
    };
    let patch = git::text(directory, &["diff", "--unified=0", base, head, "--", "*.rs"]).unwrap_or_default();
    let existing = git::text(directory, &["grep", "-h", "-o", "-E", SENDER.as_str(), base, "--", "*.rs", ":(exclude)*tests*"]).unwrap_or_default();
    let existing: BTreeSet<String> = existing.lines().filter_map(|line| line.strip_prefix("service:")).map(str::to_owned).collect();
    let needed = added_senders(&patch).into_iter().filter(|unit| !existing.contains(unit));
    needed.filter(|unit| !sources.contains(unit)).collect::<BTreeSet<_>>().into_iter().map(|unit| format!("{REGISTRY}: the alarm service {unit} has no row naming its action and owner")).collect()
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{USAGE}");
        return 2;
    };
    let refused = refusals(directory, base, head);
    for refusal in &refused {
        let _ = writeln!(out, "alarm-definitions: {refusal}");
    }
    u8::from(!refused.is_empty())
}
