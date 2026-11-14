use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use toml_edit::{DocumentMut, Item, TableLike};

use north_buck::host_class::{self, LIST};

use crate::{Range, git, holdlist};

const SELF: &str = "host-cases";
const MANIFEST: &str = "Cargo.toml";
const GENERATED: &str = "BUCK";
const THIRD_PARTY: &str = "third-party/";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Entry {
    name: String,
    test: Option<String>,
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
    let entries = tables.chain(inline).map(|case| Entry { name: field(case, "name"), test: case.get("test").and_then(Item::as_str).map(str::to_owned), reason: field(case, "reason") }).collect();
    Manifest { package: name, entries }
}

fn changed(directory: &Path, fork: &str, head: &str) -> Option<Vec<String>> {
    git::text(directory, &["diff", "--name-only", "--no-renames", fork, head]).map(|names| names.lines().map(str::to_owned).collect())
}

fn is_manifest(path: &str) -> bool {
    path.rsplit('/').next() == Some(MANIFEST)
}

/// A file a lane that adds host-case entries may change beside them: a crate's Cargo.toml, the reviewed list `build/host-cases.list`, whose change is judged apart, and the `BUCK` files `north buck targets` writes from them, which leaves out `third-party/BUCK`, held only by a gate still in shadow.
fn belongs(path: &str) -> bool {
    path == LIST || is_manifest(path) || (path.rsplit('/').next() == Some(GENERATED) && !path.starts_with(THIRD_PARTY))
}

/// `Ok(true)` is a lane the commit reviewer reads as a host-case change: one that changes `build/host-cases.list` alone, or one that adds or changes host-case entries, each of a case the landing `base`'s list already holds, beside nothing but Cargo.toml files and generated `BUCK` files. `Ok(false)` leaves the list and the entries as at the merge-base or removes entries.
pub fn check(directory: &Path, base: &str, head: &str) -> Result<bool, String> {
    let fork = git::merge_base(directory, base, head);
    let paths = changed(directory, &fork, head).ok_or_else(|| format!("git could not diff {fork}..{head}"))?;
    let show = |revision: &str, path: &str| git::text(directory, &["show", &format!("{revision}:{path}")]);
    let listed = host_class::reviewed(directory, Some(base))?;
    let (mut entries, mut refused) = (false, Vec::new());
    for path in paths.iter().filter(|path| is_manifest(path)) {
        let (before, after) = (manifest(show(&fork, path)), manifest(show(head, path)));
        for entry in after.entries.difference(&before.entries) {
            entries = true;
            let (binary, name) = host_class::member(&after.package, entry.test.as_deref(), &entry.name);
            if !listed.contains(&(binary.clone(), name.clone())) {
                refused.push(format!("crate {} names host case `{name}` of {binary} in {path}, which the landing base's {LIST} does not hold; the case enters by a change to that list alone, and its manifest entry follows once that change has landed", after.package));
            }
        }
    }
    let foreign: Vec<&str> = paths.iter().map(String::as_str).filter(|path| !belongs(path)).collect();
    if entries && !foreign.is_empty() {
        refused.push(format!("the lane also changes {}; a host-case entry lands in a change of Cargo.toml files and generated BUCK files alone", foreign.join(", ")));
    }
    let listing = paths.iter().any(|path| path == LIST);
    let beside: Vec<&str> = paths.iter().map(String::as_str).filter(|path| *path != LIST).collect();
    if listing && !beside.is_empty() {
        refused.push(format!("the lane changes {LIST} beside {}; the list changes alone, so the commit reviewer reads a change to the class by itself", beside.join(", ")));
    }
    if refused.is_empty() { Ok(entries || listing) } else { Err(refused.join("\n")) }
}

pub fn scan(directory: &Path, range: &str, state: &Path, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{SELF}: expected BASE..HEAD, got {range}");
        return 2;
    };
    let outcome = check(directory, base, head);
    let enforce = holdlist::enforcing(directory, SELF, [&git::merge_base(directory, base, head), head]);
    if !enforce {
        holdlist::record(state, SELF, (head, None), outcome.as_ref().err().map(String::as_str));
    }
    match outcome {
        Ok(true) => {
            let _ = writeln!(out, "review: host-case change");
            0
        }
        Ok(false) => 0,
        Err(refusal) => {
            let verb = if enforce { "refused" } else { "would refuse (shadow)" };
            let _ = writeln!(out, "{SELF}: {verb}: the lane cannot be admitted, since a host case enters by a change to {LIST} alone and its manifest entry follows in a later lane:");
            for line in refusal.lines() {
                let _ = writeln!(out, "  {line}");
            }
            u8::from(enforce)
        }
    }
}
