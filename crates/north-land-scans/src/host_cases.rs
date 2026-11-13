use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use toml_edit::{DocumentMut, Item, TableLike};

use crate::{Range, git, holdlist};

const SELF: &str = "host-cases";
const MANIFEST: &str = "Cargo.toml";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Entry {
    name: String,
    reason: String,
}

struct Manifest {
    package: String,
    entries: BTreeSet<Entry>,
}

fn manifest(text: Option<String>) -> Manifest {
    let document = text.and_then(|text| text.parse::<DocumentMut>().ok()).unwrap_or_default();
    let package = document.get("package");
    let name = package.and_then(|package| package.get("name")).and_then(Item::as_str).unwrap_or_default().to_owned();
    let cases = package
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("north"))
        .and_then(|north| north.get("buck"))
        .and_then(|buck| buck.get("host-cases"));
    let field = |case: &dyn TableLike, key: &str| case.get(key).and_then(Item::as_str).unwrap_or_default().to_owned();
    let tables = cases.and_then(Item::as_array_of_tables).into_iter().flatten().map(|case| case as &dyn TableLike);
    let inline = cases.and_then(Item::as_array).into_iter().flatten().filter_map(|case| case.as_inline_table().map(|case| case as &dyn TableLike));
    let entries = tables.chain(inline).map(|case| Entry { name: field(case, "name"), reason: field(case, "reason") }).collect();
    Manifest { package: name, entries }
}

fn changed(directory: &Path, fork: &str, head: &str) -> Option<Vec<String>> {
    git::text(directory, &["diff", "--name-only", "--no-renames", fork, head]).map(|names| names.lines().map(str::to_owned).collect())
}

fn is_manifest(path: &str) -> bool {
    path.rsplit('/').next() == Some(MANIFEST)
}

/// `Ok(true)` is a lane that adds or changes host-case entries in Cargo.toml files alone, which the commit reviewer reads; `Ok(false)` leaves the entries as at the merge-base or removes some.
pub fn check(directory: &Path, base: &str, head: &str) -> Result<bool, String> {
    let fork = git::merge_base(directory, base, head);
    let paths = changed(directory, &fork, head).ok_or_else(|| format!("git could not diff {fork}..{head}"))?;
    let show = |revision: &str, path: &str| git::text(directory, &["show", &format!("{revision}:{path}")]);
    let mut admitted = Vec::new();
    for path in paths.iter().filter(|path| is_manifest(path)) {
        let (before, after) = (manifest(show(&fork, path)), manifest(show(head, path)));
        for entry in after.entries.difference(&before.entries) {
            admitted.push(format!("crate {} adds or changes host case `{}` in {path}", after.package, entry.name));
        }
    }
    let foreign: Vec<&str> = paths.iter().map(String::as_str).filter(|path| !is_manifest(path)).collect();
    match (admitted.is_empty(), foreign.is_empty()) {
        (true, _) => Ok(false),
        (false, true) => Ok(true),
        (false, false) => {
            admitted.push(format!("the lane also changes {}; a host-case entry lands in a change of Cargo.toml files alone", foreign.join(", ")));
            Err(admitted.join("\n"))
        }
    }
}

pub fn scan(directory: &Path, range: &str, state: &Path, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{SELF}: expected BASE..HEAD, got {range}");
        return 2;
    };
    let outcome = check(directory, base, head);
    let enforce = holdlist::enforcing(directory, SELF, [&git::merge_base(directory, base, head), head]);
    if !enforce {
        holdlist::record(state, SELF, head, outcome.as_ref().err().map(String::as_str));
    }
    match outcome {
        Ok(true) => {
            let _ = writeln!(out, "review: host-case change");
            0
        }
        Ok(false) => 0,
        Err(refusal) => {
            let verb = if enforce { "refused" } else { "would refuse (shadow)" };
            let _ = writeln!(out, "{SELF}: {verb}: the lane cannot be admitted, since a host case lands in a change of Cargo.toml files alone:");
            for line in refusal.lines() {
                let _ = writeln!(out, "  {line}");
            }
            u8::from(enforce)
        }
    }
}
