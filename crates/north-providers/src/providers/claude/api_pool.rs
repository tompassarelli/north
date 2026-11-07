use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use north_base::platform::attribution::{self, Cause, Harness};
use toml_edit::{DocumentMut, Item};

use crate::credit::{Grant, marks};
use crate::error::{NorthError, NorthResult};
use crate::fleet::handover::AccountReadings;
use crate::fleet::limit::{park, recent, recent_within};
use crate::fleet::model::{ApiKey, Delivery, Entry, EntryKind, Phase, SessionRecord, Status, Transcript};
use crate::fleet::runtime::Runtime;
use crate::fleet::transcript::{StreamTail, stream_len, stream_path};
use crate::fleet::records::write_record;
use crate::fleet::registry::{Fleet, Halt};
use crate::fleet::sender::Sender;
use crate::foundation::clock::Timestamp;
use crate::usage::decide::world::LimitEvent;

const NEVER: &str = "9999-12-31";
const LOW_CREDIT: &str = "credit balance is too low";
const ENDED_RECENT: Duration = Duration::from_secs(6 * 3600);
const ACCOUNT_SINCE: &str = "account-since";

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
    /// The earliest expiry of a grant a launch may draw on that is not yet past on `today`, or `NEVER` when no grant is dated; an entry whose dated grants all expired has none.
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

fn pool(policy: &Path, home: &Path, state_root: &Path, here: &str) -> Vec<Org> {
    let document: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    declared(&document, &crate::credit::open_grants(policy, &state_root.join("ledger.db"), &today()), home, here)
}

fn readable(file: &Path) -> bool {
    std::fs::metadata(file).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
}

/// One line per pool entry on `here`, then the entry a launch on the pooled account takes: what `north fleet accounts --api` prints.
pub fn listing(policy: &Path, home: &Path, state_root: &Path, here: &str) -> Vec<String> {
    let orgs = pool(policy, home, state_root, here);
    let marks = state_root.join("api-exhausted");
    let mut lines: Vec<String> = orgs.iter().map(|org| format!("{} keys={} exhausted={}", org.line(), org.keys.len(), exhausted(&marks, org))).collect();
    let chosen = open(&orgs, &today(), |org| exhausted(&marks, org), readable);
    lines.push(chosen.map_or_else(|| "api launches on: no open entry".to_owned(), |(org, _)| format!("api launches on: {}", org.name)));
    lines
}

/// The entry and key a run outside the fleet takes: the one the pool opens next, leaving out the organizations in `skipped`.
pub fn next_key(policy: &Path, home: &Path, state_root: &Path, here: &str, skipped: &[String]) -> Option<(String, ApiKey)> {
    let orgs = pool(policy, home, state_root, here);
    let marks = state_root.join("api-exhausted");
    open(&orgs, &today(), |org| skipped.contains(&org.id) || exhausted(&marks, org), readable).map(|(org, key)| (org.id.clone(), key))
}

/// Marks organization `id` exhausted on `session`'s report that the API refused a turn for a credit balance too low.
pub fn mark_ran_out(policy: &Path, state_root: &Path, id: &str, session: &str) -> std::io::Result<bool> {
    marks::mark(&state_root.join("api-exhausted"), id, session, &today(), crate::credit::next_credits(policy).get(id).map(String::as_str))
}

/// Whether an API error message is the low-credit refusal.
pub fn low_credit(message: &str) -> bool {
    message.to_lowercase().contains(LOW_CREDIT)
}

/// Whether a transcript's newest ended turn ended on the API's low-credit error, even when a message delivered since has started another turn.
pub fn ran_out(entries: &[Entry]) -> bool {
    let Some(end) = entries.iter().rposition(|entry| entry.kind == EntryKind::TurnEnd) else { return false };
    end.checked_sub(1).and_then(|error| entries.get(error)).is_some_and(|error| error.kind == EntryKind::Error && low_credit(&error.text))
}

/// The transcript of `conversation` in the host stream at `stream` from byte `since` on: the turns a session ran on the account it moved onto there.
pub fn since(runtime: &dyn Runtime, stream: &Path, since: u64, conversation: Option<&str>) -> Transcript {
    let mut tail = StreamTail::open(stream.to_owned(), since);
    let mut transcript = Transcript::default();
    while let Ok(frames) = tail.frames() && !frames.is_empty() {
        for frame in frames.iter().filter(|frame| runtime.frame_conversation(frame) == conversation) {
            runtime.observe(frame, &mut transcript);
        }
    }
    transcript
}

/// The session's transcript on its current account: since the byte of its host stream where the last move put it on that account, or its recent transcript when it has not moved on this host.
fn on_account(fleet: &Fleet, runtime: &dyn Runtime, record: &SessionRecord) -> Option<Transcript> {
    let moved = fleet.session_dir(&record.name).ok().and_then(|dir| std::fs::read_to_string(dir.join(ACCOUNT_SINCE)).ok());
    let offset = moved.as_deref().and_then(|text| text.trim_end().split_once(' ')).filter(|(_, host_dir)| Path::new(host_dir) == record.host_dir).and_then(|(offset, _)| offset.parse().ok());
    match offset {
        Some(offset) => Some(since(runtime, &stream_path(&record.host_dir), offset, record.conversation.as_deref())),
        None => fleet.follow_recent(&record.name).ok().map(|follower| follower.transcript().clone()),
    }
}

/// The account a session on `account` (`LABEL:ORG`) moves to once ORG ran out of credit: the pinned account of the entry the pool opens without ORG and the entries `exhausted` marks.
pub fn moved_to(account: &str, orgs: &[Org], today: &str, exhausted: impl Fn(&Org) -> bool) -> Option<String> {
    let (label, spent) = account.split_once(':')?;
    open(orgs, today, |org| org.name == spent || exhausted(org), |_| true).map(|(org, _)| format!("{label}:{}", org.name))
}

/// What a fleet tick reads of the pool: each organization's grants a launch may still draw on, by id, each one's next credit day, and today.
pub struct Reading {
    pub grants: BTreeMap<String, Vec<Grant>>,
    pub next_credits: BTreeMap<String, String>,
    pub today: String,
}

impl Reading {
    fn of(policy: &Path, state: &Path) -> Self {
        let today = today();
        Self { grants: crate::credit::open_grants(policy, &state.join("ledger.db"), &today), next_credits: crate::credit::next_credits(policy), today }
    }
}

/// Marks exhausted the organization of each recent session on an `api:ORG` account whose newest turn on that account ended on the API's low-credit error, the
/// only sign an organization is spent; then moves that session, ended or not, and each session still running and not mid-turn on an organization marked exhausted, onto the entry the pool opens next, on its conversation;
/// when no entry is open a refused session moves to the subscription account whose window resets first and parks there until it does. One line per mark and move.
pub fn move_spent(fleet: &Fleet, runtime: &dyn Runtime, policy: &Path, home: &Path, readings: &dyn AccountReadings) -> Vec<String> {
    let reading = Reading::of(policy, &home.join(".local/state/north"));
    let observed = std::cell::OnceCell::new();
    let subscription = |record: &SessionRecord, machine: &str| {
        let now = crate::fleet::runtime::now_ms();
        let labels: Vec<String> = fleet
            .accounts()
            .into_iter()
            .filter(|account| account.api.is_none() && account.provider.label() == record.provider.label())
            .map(|account| account.label)
            .filter(|label| super::accounts::on_machine(&record.provider, policy, home, label, machine).is_ok())
            .collect();
        let limits = &observed.get_or_init(|| readings.observed(now)).limits;
        next_subscription(labels.iter().map(String::as_str), limits, now).map(|(label, resumes)| (label.to_owned(), resumes))
    };
    settle(fleet, runtime, (policy, home), &reading, subscription, |record, spent, to, machine, failed, resumes| move_onto(fleet, record, spent, (to, policy, home, machine), (failed, resumes)))
}

/// The subscription account to move a session onto once every API org is spent, and when its window resets: an account no reading holds full comes first with no reset, else the one whose full windows lift soonest.
pub fn next_subscription<'a>(labels: impl IntoIterator<Item = &'a str>, limits: &[LimitEvent], now_ms: u64) -> Option<(&'a str, Option<u64>)> {
    let lifts = |label: &str| limits.iter().filter(|limit| limit.account.0 == label).filter_map(|limit| u64::try_from(limit.resets_at?.0).ok()).map(|at| at * 1000).filter(|at| *at > now_ms).max();
    labels.into_iter().map(|label| (label, lifts(label))).min_by_key(|(label, resumes)| (resumes.unwrap_or(0), *label))
}

fn settle(
    fleet: &Fleet,
    runtime: &dyn Runtime,
    (policy, home): (&Path, &Path),
    reading: &Reading,
    mut subscription: impl FnMut(&SessionRecord, &str) -> Option<(String, Option<u64>)>,
    mut moving: impl FnMut(&SessionRecord, &str, &str, &str, bool, Option<u64>) -> String,
) -> Vec<String> {
    let document: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let Reading { grants, next_credits, today } = reading;
    let until = |org: &Org| next_credits.get(&org.id).map(String::as_str);
    let dir = home.join(".local/state/north/api-exhausted");
    let mut lines = Vec::new();
    let sessions = fleet.sessions().unwrap_or_default().into_iter().filter(|record| !matches!(record.phase(), Some(Phase::Stopped | Phase::Retired | Phase::Adopted)));
    for record in sessions.filter(|record| if record.ended() { recent_within(record, ENDED_RECENT) } else { recent(record) }) {
        let Some((_, name)) = record.account.split_once(':') else { continue };
        let machine = record.machine.as_ref().map_or_else(crate::platform::host::name, |machine| machine.name.clone());
        let orgs = declared(&document, grants, home, &machine);
        let Some(org) = orgs.iter().find(|org| org.name == name) else { continue };
        let Some(transcript) = on_account(fleet, runtime, &record) else { continue };
        let failed = ran_out(&transcript.entries);
        if !failed && (record.ended() || transcript.status == Status::Working || !exhausted(&dir, org)) {
            continue;
        }
        let marked = if failed { marks::mark(&dir, &org.id, &record.name, today, until(org)) } else { Ok(false) };
        match &marked {
            Ok(true) => lines.push(format!("api:{} marked exhausted: {}'s last turn ended on a credit balance too low", org.name, record.name)),
            Ok(false) => {}
            Err(error) => lines.push(format!("api:{} ran out of credit but its exhausted mark was not written: {error}", org.name)),
        }
        let next = match moved_to(&record.account, &orgs, today, |org| exhausted(&dir, org)) {
            Some(to) => Some((to, None)),
            None if failed => subscription(&record, &machine),
            None => None,
        };
        match next {
            Some((to, resumes)) => lines.push(moving(&record, &org.name, &to, &machine, failed, resumes)),
            None if matches!(marked, Ok(true)) => lines.push(format!("{}: api:{} ran out of credit and no other api org or subscription account is open, so it stays parked", record.name, org.name)),
            None => {}
        }
    }
    lines
}

fn move_onto(fleet: &Fleet, record: &SessionRecord, spent: &str, (to, policy, home, machine): (&str, &Path, &Path, &str), (failed, resumes): (bool, Option<u64>)) -> String {
    let moved = || -> NorthResult<()> {
        super::accounts::on_machine(&record.provider, policy, home, to, machine)?;
        switch(fleet, record, to, |name| fleet.revive(name, None).map(drop))?;
        fleet.waits().clear_usage(&record.name)?;
        attribution::record(&record.name, "fleet", Cause::Harness(Harness::Capacity), &format!("moved from account {} to {to}: org {spent} ran out of credit", record.account));
        if let Some(until_ms) = resumes {
            return park(&fleet.waits(), &fleet.session(&record.name)?, until_ms).map(drop);
        }
        if !failed {
            return Ok(());
        }
        fleet.send_spooled(&Sender::fleet(), &record.name, &resume_message(spent, to), Delivery::Queued)
    };
    match moved() {
        Ok(()) => match resumes {
            Some(until_ms) => format!("{}: moved from account {} to {to} on its conversation, since org {spent} ran out of credit, and parked until {to}'s window resets at {}", record.name, record.account, Timestamp::from_unix_millis(i64::try_from(until_ms).unwrap_or_default()).iso_date()),
            None => format!("{}: moved from account {} to {to} on its conversation, since org {spent} ran out of credit", record.name, record.account),
        },
        Err(error) => format!("{}: not moved from account {} to {to}: {}", record.name, record.account, error.user_message()),
    }
}

/// Restarts `record` on its conversation under `to`; a restart that fails puts the session back on its account and running, so the next tick tries the move again instead of finding it stopped half moved.
fn switch(fleet: &Fleet, record: &SessionRecord, to: &str, restart: impl Fn(&str) -> NorthResult<()>) -> NorthResult<()> {
    fleet.halt(&record.name, Halt::MovedAccount)?;
    set_account(fleet, &record.name, to)?;
    let (since, host_dir) = (fleet.session_dir(&record.name)?.join(ACCOUNT_SINCE), fleet.session(&record.name)?.host_dir);
    std::fs::write(&since, format!("{} {}\n", stream_len(&host_dir), host_dir.display()))?;
    let Err(error) = restart(&record.name) else { return Ok(()) };
    let _ = std::fs::remove_file(&since);
    let restored = set_account(fleet, &record.name, &record.account).and_then(|()| restart(&record.name));
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
