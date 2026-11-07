use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item};

use crate::credit::Grant;
use crate::fleet::limit::recent;
use crate::fleet::model::{ApiKey, Entry, EntryKind};
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;

const NEVER: &str = "9999-12-31";
const LOW_CREDIT: &str = "credit balance is too low";

/// One orgs.iter().find(|org| org.name == name)anization's entry in the pool of API keys: identified by its organization id, so two keys
/// that report the same id are one entry with one balance and one exhausted mark.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Org {
    pub name: String,
    pub id: String,
    pub emails: Vec<String>,
    pub keys: Vec<PathBuf>,
    pub expires: Vec<String>,
}

impl Org {
    /// The earliest credit expiry not yet past on `today`, or `NEVER` when none is dated; an entry whose dated credit has all expired has none left.
    fn sorts_by(&self, today: &str) -> Option<&str> {
        let live = self.expires.iter().map(String::as_str).filter(|date| *date >= today).min();
        match (live, self.expires.is_empty()) {
            (Some(date), _) => Some(date),
            (None, true) => Some(NEVER),
            (None, false) => None,
        }
    }

    fn line(&self) -> String {
        format!("org={} id={} emails={} expires={}", self.name, self.id, self.emails.join(","), if self.expires.is_empty() { "unknown".to_owned() } else { self.expires.join(",") })
    }
}

/// The pool entries `[accounts.claude.api_orgs.NAME]` declares for `here`, entries sharing an id merged, each dated by its organization's `grants`.
pub fn declared(policy: &DocumentMut, grants: &BTreeMap<String, Vec<Grant>>, home: &Path, here: &str) -> Vec<Org> {
    let table = policy.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("api_orgs")).and_then(Item::as_table_like);
    let strings = |item: Option<&Item>| -> Vec<String> { item.and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect() };
    let mut merged: BTreeMap<String, Org> = BTreeMap::new();
    for (name, entry) in table.into_iter().flat_map(|table| table.iter()) {
        let Some(id) = entry.get("id").and_then(Item::as_str) else { continue };
        let org = merged.entry(id.to_owned()).or_insert_with(|| Org { name: name.to_owned(), id: id.to_owned(), ..Org::default() });
        org.emails.extend(strings(entry.get("emails")));
        org.keys.extend(entry.get("keys").and_then(|keys| keys.get(here)).and_then(Item::as_array).into_iter().flatten().filter_map(|file| file.as_str().map(|path| expand(home, path))));
    }
    for org in merged.values_mut() {
        org.expires = grants.get(&org.id).into_iter().flatten().map(|grant| grant.expires.clone()).collect();
    }
    merged.into_values().collect()
}

fn expand(home: &Path, path: &str) -> PathBuf {
    path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| home.join(rest))
}

/// The entry to launch on: the one whose credit expires soonest among those not marked exhausted and holding a key `usable` reads, with that key.
pub fn open<'a>(orgs: &'a [Org], today: &str, exhausted: impl Fn(&Org) -> bool, usable: impl Fn(&Path) -> bool) -> Option<(&'a Org, ApiKey)> {
    let mut ranked: Vec<(&str, &Org)> = orgs.iter().filter(|org| !exhausted(org)).filter_map(|org| Some((org.sorts_by(today)?, org))).collect();
    ranked.sort_by(|(left, a), (right, b)| left.cmp(right).then_with(|| a.name.cmp(&b.name)));
    ranked.into_iter().find_map(|(_, org)| Some((org, ApiKey { org: org.name.clone(), file: org.keys.iter().find(|file| usable(file))?.clone() })))
}

/// Whether `org` is marked exhausted: a file named by its organization id in `marks`.
pub fn exhausted(marks: &Path, org: &Org) -> bool {
    marks.join(&org.id).exists()
}

/// Today's UTC date as `YYYY-MM-DD`.
pub fn today() -> String {
    Timestamp::now().iso_date()
}

/// One line per pool entry on `here`, then the entry a launch on the pooled account takes: what `north fleet accounts --api` prints.
pub fn listing(policy: &Path, home: &Path, state_root: &Path, here: &str) -> Vec<String> {
    let document: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let orgs = declared(&document, &crate::credit::grants(policy), home, here);
    let marks = state_root.join("api-exhausted");
    let mut lines: Vec<String> = orgs.iter().map(|org| format!("{} keys={} exhausted={}", org.line(), org.keys.len(), exhausted(&marks, org))).collect();
    let chosen = open(&orgs, &today(), |org| exhausted(&marks, org), |file| std::fs::metadata(file).is_ok_and(|meta| meta.is_file() && meta.len() > 0));
    lines.push(chosen.map_or_else(|| "api launches on: no open entry".to_owned(), |(org, _)| format!("api launches on: {}", org.name)));
    lines
}

/// Marks exhausted the organization of each recent session on an `api:ORG` account whose last turn ended on the
/// API's low-credit error, so the next launch on the pooled account takes the next entry; one line per new mark.
pub fn mark_spent(fleet: &Fleet, policy: &Path, home: &Path) -> Vec<String> {
    let orgs = super::accounts::declared(policy, home).orgs;
    let marks = home.join(".local/state/north/api-exhausted");
    let marks = marks.as_path();
    let mut lines = Vec::new();
    for record in fleet.sessions().unwrap_or_default().into_iter().filter(recent) {
        let Some(org) = record.account.strip_prefix("api:").and_then(|name| orgs.iter().find(|org| org.name == name)) else { continue };
        if exhausted(marks, org) {
            continue;
        }
        let Ok(follower) = fleet.follow_recent(&record.name) else { continue };
        let transcript = follower.transcript();
        let tail: Vec<&Entry> = transcript.entries.iter().rev().take(2).collect();
        let spent = matches!(tail.as_slice(), [end, error] if end.kind == EntryKind::TurnEnd && error.kind == EntryKind::Error && error.text.to_lowercase().contains(LOW_CREDIT));
        if !spent {
            continue;
        }
        match std::fs::create_dir_all(marks).and_then(|()| std::fs::write(marks.join(&org.id), format!("{} {}\n", record.name, today()))) {
            Ok(()) => lines.push(format!("api:{} marked exhausted: {}'s last turn ended on a credit balance too low", org.name, record.name)),
            Err(error) => eprintln!("north fleet: api:{} ran out of credit but its exhausted mark was not written: {error}", org.name),
        }
    }
    lines
}
