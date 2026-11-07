use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use north_base::platform::attribution::{self, Cause, Harness};
use toml_edit::{DocumentMut, Item};

use crate::credit::{Grant, marks};
use crate::error::{NorthError, NorthResult};
use crate::fleet::limit::recent;
use crate::fleet::model::{ApiKey, Delivery, Entry, EntryKind, SessionRecord};
use crate::fleet::records::write_record;
use crate::fleet::registry::{Fleet, Halt};
use crate::fleet::sender::Sender;
use crate::foundation::clock::Timestamp;

const NEVER: &str = "9999-12-31";
const LOW_CREDIT: &str = "credit balance is too low";

/// One organization's entry in the pool of API keys: identified by its organization id, so two keys
/// that report the same id are one entry with one balance and one exhausted mark.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Org {
    pub name: String,
    pub id: String,
    pub emails: Vec<String>,
    pub keys: Vec<PathBuf>,
    pub expires: Option<Vec<String>>,
}

impl Org {
    /// The earliest expiry of a grant with credit left that is not yet past on `today`, or `NEVER` when no grant is dated; an entry whose dated grants are all spent or expired has none left.
    fn sorts_by(&self, today: &str) -> Option<&str> {
        match &self.expires {
            None => Some(NEVER),
            Some(dates) => dates.iter().map(String::as_str).filter(|date| *date >= today).min(),
        }
    }

    fn line(&self) -> String {
        let expires = match &self.expires {
            None => "unknown".to_owned(),
            Some(dates) if dates.is_empty() => "none left".to_owned(),
            Some(dates) => dates.join(","),
        };
        format!("org={} id={} emails={} expires={expires}", self.name, self.id, self.emails.join(","))
    }
}

/// The pool entries `[accounts.claude.api_orgs.NAME]` declares for `here`, entries sharing an id merged, each dated by the grants with credit left that `grants` holds for its organization.
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
        org.expires = grants.get(&org.id).map(|open| open.iter().map(|grant| grant.expires.clone()).collect());
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

/// Whether `org` is marked exhausted today: a mark named by its organization id under `dir` that holds.
pub fn exhausted(dir: &Path, org: &Org) -> bool {
    marks::held(dir, &org.id, &today())
}

/// Today's UTC date as `YYYY-MM-DD`.
pub fn today() -> String {
    Timestamp::now().iso_date()
}

/// One line per pool entry on `here`, then the entry a launch on the pooled account takes: what `north fleet accounts --api` prints.
pub fn listing(policy: &Path, home: &Path, state_root: &Path, here: &str) -> Vec<String> {
    let document: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let orgs = declared(&document, &crate::credit::open_grants(policy, &state_root.join("ledger.db"), &today()), home, here);
    let marks = state_root.join("api-exhausted");
    let mut lines: Vec<String> = orgs.iter().map(|org| format!("{} keys={} exhausted={}", org.line(), org.keys.len(), exhausted(&marks, org))).collect();
    let chosen = open(&orgs, &today(), |org| exhausted(&marks, org), |file| std::fs::metadata(file).is_ok_and(|meta| meta.is_file() && meta.len() > 0));
    lines.push(chosen.map_or_else(|| "api launches on: no open entry".to_owned(), |(org, _)| format!("api launches on: {}", org.name)));
    lines
}

/// Whether a transcript's last turn ended on the API's low-credit error.
pub fn ran_out(entries: &[Entry]) -> bool {
    let tail: Vec<&Entry> = entries.iter().rev().take(2).collect();
    matches!(tail.as_slice(), [end, error] if end.kind == EntryKind::TurnEnd && error.kind == EntryKind::Error && error.text.to_lowercase().contains(LOW_CREDIT))
}

/// The account a session on `account` (`LABEL:ORG`) moves to once ORG ran out of credit: the pinned account of the entry the pool opens without ORG and the entries `exhausted` marks.
pub fn moved_to(account: &str, orgs: &[Org], today: &str, exhausted: impl Fn(&Org) -> bool) -> Option<String> {
    let (label, spent) = account.split_once(':')?;
    open(orgs, today, |org| org.name == spent || exhausted(org), |_| true).map(|(org, _)| format!("{label}:{}", org.name))
}

/// Marks exhausted the organization of each recent session on an `api:ORG` account whose last turn ended on the API's
/// low-credit error, then moves the session onto the entry the pool opens next, on its conversation; one line per mark and move.
pub fn move_spent(fleet: &Fleet, policy: &Path, home: &Path) -> Vec<String> {
    let document: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let (today, state) = (today(), home.join(".local/state/north"));
    let grants = crate::credit::open_grants(policy, &state.join("ledger.db"), &today);
    let next_credits = crate::credit::next_credits(policy);
    let dir = state.join("api-exhausted");
    let mut lines = Vec::new();
    for record in fleet.sessions().unwrap_or_default().into_iter().filter(|record| !record.stopped && recent(record)) {
        let Some((_, name)) = record.account.split_once(':') else { continue };
        let machine = record.machine.as_ref().map_or_else(crate::platform::host::name, |machine| machine.name.clone());
        let orgs = declared(&document, &grants, home, &machine);
        let Some(org) = orgs.iter().find(|org| org.name == name) else { continue };
        if !fleet.follow_recent(&record.name).is_ok_and(|follower| ran_out(&follower.transcript().entries)) {
            continue;
        }
        let marked = marks::mark(&dir, &org.id, &record.name, &today, next_credits.get(&org.id).map(String::as_str));
        match &marked {
            Ok(true) => lines.push(format!("api:{} marked exhausted: {}'s last turn ended on a credit balance too low", org.name, record.name)),
            Ok(false) => {}
            Err(error) => lines.push(format!("api:{} ran out of credit but its exhausted mark was not written: {error}", org.name)),
        }
        match moved_to(&record.account, &orgs, &today, |org| exhausted(&dir, org)) {
            Some(to) => lines.push(move_onto(fleet, &record, &org.name, (&to, policy, home, &machine))),
            None if matches!(marked, Ok(true)) => lines.push(format!("{}: api:{} ran out of credit and no other api org is open, so it stays parked", record.name, org.name)),
            None => {}
        }
    }
    lines
}

fn move_onto(fleet: &Fleet, record: &SessionRecord, spent: &str, (to, policy, home, machine): (&str, &Path, &Path, &str)) -> String {
    let moved = || -> NorthResult<()> {
        super::accounts::on_machine(&record.provider, policy, home, to, machine)?;
        switch(fleet, record, to)?;
        fleet.waits().clear_usage(&record.name)?;
        attribution::record(&record.name, "fleet", Cause::Harness(Harness::Capacity), &format!("moved from account {} to {to}: org {spent} ran out of credit", record.account));
        fleet.send_signed(&Sender::fleet(), &record.name, &resume_message(spent, to), Delivery::Queued)
    };
    match moved() {
        Ok(()) => format!("{}: moved from account {} to {to} on its conversation, since org {spent} ran out of credit", record.name, record.account),
        Err(error) => format!("{}: not moved from account {} to {to}: {}", record.name, record.account, error.user_message()),
    }
}

/// Restarts `record` on its conversation under `to`; a restart that fails puts the session back on its account and running, so the next tick tries the move again instead of finding it stopped half moved.
fn switch(fleet: &Fleet, record: &SessionRecord, to: &str) -> NorthResult<()> {
    fleet.halt(&record.name, Halt::Relaunched)?;
    set_account(fleet, &record.name, to)?;
    let Err(error) = fleet.revive(&record.name, None) else { return Ok(()) };
    let restored = set_account(fleet, &record.name, &record.account).and_then(|()| fleet.revive(&record.name, None));
    Err(match restored {
        Ok(_) => error,
        Err(left) => NorthError::State(format!("{}; it stays stopped on {}, which did not restart either: {}", error.user_message(), record.account, left.user_message())),
    })
}

fn set_account(fleet: &Fleet, name: &str, account: &str) -> NorthResult<()> {
    let mut record = fleet.session(name)?;
    account.clone_into(&mut record.account);
    write_record(&fleet.session_dir(name)?, &record)
}

pub fn resume_message(spent: &str, to: &str) -> String {
    format!("[north: api org {spent} ran out of credit, so this session now runs on {to}] Your last turn ended on the API's credit balance too low error before it did any work. Carry on with that turn from where it stopped.")
}

#[cfg(test)]
#[path = "api_pool_tests.rs"]
mod tests;
