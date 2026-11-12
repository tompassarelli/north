//! `north issue convert`: the dry run that turns one issue's open checkboxes into a manifest of the sub-issues they
//! would become (north#1137), and the listing of the unticked boxes closed issues still carry. Neither edits GitHub.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::foundation::clock::Timestamp;
use crate::platform::betterleaks;
use crate::threads::claims;
use manifest::{Manifest, Parent, Row, Scan, Tracker};

mod boxes;
mod closed;
mod manifest;
mod read;
#[cfg(test)]
mod tests;

const OWNER: &str = north_github_mirror::store::OWNER;
const DENYLIST_UNDER_HOME: &str = "code/south/main/privacy/public-denylist.toml";
const MANIFESTS_UNDER_HOME: &str = ".local/state/north/convert";

const USAGE: &str = "usage: north issue convert [OWNER/]REPO#N --dry-run [--manifest FILE]
       north issue convert [OWNER/]REPO --closed-boxes

--dry-run writes a manifest with one row per unticked checkbox of the issue: its verdict (leaf, third-level issue, prose,
gate, held by a live claim), title, type, labels and body. It creates nothing. A row is flagged, and the command exits 1,
when its title is empty, longer than 80 characters, starts with a letter code or equals an open issue's, when a leaf has
no Done when command in backticks and no Verify: or Category: line, when the repository lacks its type or a label, or
when the public deny-list matches its text in a public repository; the manifest is refused when its rows do not add up
to the body's unticked checkboxes. A title is the box's own first clause when that is 15 to 80 characters, else empty
for a person to write. Run again on an edited manifest and it keeps each row's verdict, title, type and labels.
The manifest is FILE, else ~/.local/state/north/convert/REPO-N.json.

--closed-boxes lists every unticked checkbox on a closed issue of the repository with the open issue that holds the
same text, or `no copy`. It edits nothing.

Either mode reads the repository from GitHub once.";

enum Asked {
    Help,
    DryRun { owner: String, name: String, number: u64, manifest: Option<PathBuf> },
    ClosedBoxes { owner: String, name: String },
}

fn slug(target: &str) -> Result<(String, String), String> {
    let (owner, name) = target.split_once('/').unwrap_or((OWNER, target));
    let named = |text: &str| !text.is_empty() && text.chars().all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '-' | '_' | '.'));
    if named(owner) && named(name) { Ok((owner.to_owned(), name.to_owned())) } else { Err(format!("{target} is not [OWNER/]REPO")) }
}

fn dry(target: &str, manifest: Option<&str>) -> Result<Asked, String> {
    let (repository, number) = target.split_once('#').ok_or_else(|| format!("{target} is not [OWNER/]REPO#N"))?;
    let number = number.parse().map_err(|_| format!("{target} is not [OWNER/]REPO#N"))?;
    let (owner, name) = slug(repository)?;
    Ok(Asked::DryRun { owner, name, number, manifest: manifest.map(PathBuf::from) })
}

fn asked(arguments: &[String]) -> Result<Asked, String> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["-h" | "--help"] | ["convert", "-h" | "--help"] => Ok(Asked::Help),
        ["convert", target, "--closed-boxes"] => slug(target).map(|(owner, name)| Asked::ClosedBoxes { owner, name }),
        ["convert", target, "--dry-run"] => dry(target, None),
        ["convert", target, "--dry-run", "--manifest", file] | ["convert", target, "--manifest", file, "--dry-run"] => dry(target, Some(*file)),
        ["convert", _, ..] => Err("only --dry-run and --closed-boxes are built; nothing here creates an issue".to_owned()),
        _ => Err("name what to convert".to_owned()),
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let done = match asked(arguments) {
        Ok(Asked::Help) => {
            println!("{USAGE}");
            return 0;
        }
        Ok(Asked::DryRun { owner, name, number, manifest }) => dry_run(&owner, &name, number, manifest),
        Ok(Asked::ClosedBoxes { owner, name }) => closed_boxes(&owner, &name).map(|()| true),
        Err(why) => {
            eprintln!("north issue convert: {why}\n{USAGE}");
            return 2;
        }
    };
    match done {
        Ok(passed) => u8::from(!passed),
        Err(why) => {
            eprintln!("north issue convert: {why}");
            1
        }
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// The box slug of each claim on `item` that still holds it, None for a claim on the whole issue. No fleet is read, so
/// a claim whose session died holds its box until it goes stale: the box stays in the body, which loses nothing.
fn live_claims(item: &str, opened: i64) -> Result<Vec<Option<String>>, String> {
    let database = crate::threads::client::threads_database();
    if !database.is_file() {
        println!("claims: no threads ledger at {}, so no box is read as held", database.display());
        return Ok(Vec::new());
    }
    let now = Timestamp::now().unix_seconds();
    let held = claims::ledger::claims(&claims::ledger::open(&database, false)?)?;
    Ok(held.into_iter().filter(|claim| claim.item == item && claim.standing(claims::Issue::Open { opened }, None, now) == claims::Standing::Live).map(|claim| claim.slug).collect())
}

/// The landing's deny-list scan over a row's text; a private repository's rows are not scanned, as its landings are not.
fn scan(private: bool) -> Box<Scan> {
    if private {
        return Box::new(|_: &str| Ok(BTreeSet::new()));
    }
    let denylist = home().join(DENYLIST_UNDER_HOME);
    Box::new(move |text: &str| if denylist.is_file() { betterleaks::scan_text(&denylist, text) } else { Err(format!("the public deny-list {} is missing", denylist.display())) })
}

fn reviewed(file: &Path, reference: &str) -> Result<Vec<Row>, String> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", file.display())),
    };
    let manifest: Manifest = serde_json::from_str(&text).map_err(|error| format!("{} is not a manifest this command wrote ({error}); move it away or name another with --manifest", file.display()))?;
    if manifest.issue != reference {
        return Err(format!("{} is the manifest of {}, not of {reference}; name another with --manifest", file.display(), manifest.issue));
    }
    Ok(manifest.rows)
}

fn dry_run(owner: &str, name: &str, number: u64, file: Option<PathBuf>) -> Result<bool, String> {
    let repository = read::repository(owner, name, false)?;
    let reference = format!("{owner}/{name}#{number}");
    let issue = repository.issues.iter().find(|issue| issue.number == number).ok_or_else(|| format!("{reference} is not an open issue"))?;
    let file = file.unwrap_or_else(|| home().join(MANIFESTS_UNDER_HOME).join(format!("{name}-{number}.json")));
    let reviewed = reviewed(&file, &reference)?;
    let claimed = live_claims(&format!("{name}#{number}"), issue.created)?;
    let parent = Parent { reference: &reference, body: &issue.body, labels: &issue.labels, children: issue.children };
    let titles: BTreeSet<String> = repository.issues.iter().map(|issue| manifest::title_key(&issue.title)).collect();
    let scan = scan(repository.private);
    let mut manifest = manifest::draft(&parent, &claimed, &reviewed);
    manifest.check(&parent, &Tracker { titles: &titles, labels: repository.labels.as_ref(), scan: &*scan });
    let text = serde_json::to_string_pretty(&manifest).map_err(|error| error.to_string())? + "\n";
    if let Some(directory) = file.parent().filter(|directory| !directory.as_os_str().is_empty()) {
        std::fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    }
    std::fs::write(&file, &text).map_err(|error| format!("{}: {error}", file.display()))?;
    println!("{}", manifest.count_line());
    manifest.refused.iter().for_each(|refusal| println!("refused: {refusal}"));
    manifest.flagged().for_each(|row| println!("row {} ({}): {}", row.row, row.verdict.label(), row.flags.join("; ")));
    let hash: String = Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    println!("manifest {} sha256 {hash}", file.display());
    Ok(manifest.passes())
}

fn closed_boxes(owner: &str, name: &str) -> Result<(), String> {
    let repository = read::repository(owner, name, true)?;
    let bodies = |open: bool| repository.issues.iter().filter(|issue| issue.open == open).map(|issue| (issue.number, issue.body.as_str())).collect::<Vec<_>>();
    let found = closed::leftovers(&bodies(false), &bodies(true));
    for leftover in &found {
        match leftover.copy {
            Some(copy) => println!("{name}#{} -> {name}#{} ({}): {}", leftover.issue, copy.issue, if copy.ticked { "ticked there" } else { "open there" }, leftover.text),
            None => println!("{name}#{} no copy: {}", leftover.issue, leftover.text),
        }
    }
    let issues: BTreeSet<u64> = found.iter().map(|leftover| leftover.issue).collect();
    let copies = found.iter().filter(|leftover| leftover.copy.is_some()).count();
    println!("{owner}/{name}: {} closed issues hold {} unticked boxes: {copies} with a copy on an open issue, {} with no copy", issues.len(), found.len(), found.len() - copies);
    Ok(())
}
