use super::sender::Sender;
use super::store::{Addressee, Exchange, Exchanged, FleetRow, Registration, RowKind, SessionRow, WaitKey};
use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    Spoofed(String),
    Unregistered(String),
}

impl Refusal {
    pub fn why(&self) -> &str {
        match self {
            Self::Spoofed(why) | Self::Unregistered(why) => why,
        }
    }
}

pub fn admit(row: &FleetRow, peer: &str, ledger: &[SessionRow]) -> Result<FleetRow, Refusal> {
    if row.from != peer {
        return Err(Refusal::Spoofed(format!("row {} names from {} but came from {peer}", row.id, row.from)));
    }
    if let Sender::Session(sender) = &row.sender {
        on_peer(ledger, peer, sender).map_err(|held| refusal(held, format!("row {} names sender {sender}, which is not a session on {peer}", row.id)))?;
    }
    if row.kind != RowKind::Message {
        on_peer(ledger, peer, &row.to).map_err(|held| refusal(held, format!("row {} is a {} for {}, which is not a session on {peer}", row.id, row.kind.label(), row.to)))?;
    }
    let urgent = row.urgent && on_peer(ledger, peer, &row.to).is_ok();
    Ok(FleetRow { origin: Some(peer.to_owned()), urgent, ..row.clone() })
}

fn on_peer(ledger: &[SessionRow], peer: &str, name: &str) -> Result<(), bool> {
    match ledger.iter().find(|session| session.name == name) {
        Some(session) if runs_on(session, peer) => Ok(()),
        Some(_) => Err(true),
        None => Err(false),
    }
}

fn runs_on(session: &SessionRow, peer: &str) -> bool {
    session.host == peer || session.machine.as_deref() == Some(peer)
}

fn may_hold(ledger: &[SessionRow], peer: &str, name: &str) -> bool {
    match ledger.iter().find(|session| session.name == name) {
        Some(session) => session.host == peer,
        None => name.strip_prefix(peer).is_some_and(|rest| rest.len() > 1 && rest.starts_with('-')),
    }
}

fn iso(ms: u64) -> String {
    Timestamp::from_unix_millis(i64::try_from(ms).unwrap_or(i64::MAX)).iso()
}

fn stamped(session: SessionRow, ledger: &[SessionRow], now_ms: u64) -> SessionRow {
    let created = ledger.iter().find(|held| held.name == session.name).map_or_else(|| iso(now_ms), |held| held.created.clone());
    SessionRow { created, ..session }
}

fn floored(addressee: Addressee, ledger: &[SessionRow], now_ms: u64) -> Addressee {
    let created = ledger.iter().find(|session| session.name == addressee.name).and_then(|session| Timestamp::parse(&session.created).ok());
    let floor = created.and_then(|created| u64::try_from(created.unix_millis()).ok()).unwrap_or(now_ms);
    Addressee { since_ms: addressee.since_ms.max(floor), control_since_ms: addressee.control_since_ms.map(|since| since.max(floor)), ..addressee }
}

fn refusal(held_elsewhere: bool, why: String) -> Refusal {
    if held_elsewhere { Refusal::Spoofed(why) } else { Refusal::Unregistered(why) }
}

pub fn admit_all(rows: &[FleetRow], peer: &str, ledger: &[SessionRow]) -> NorthResult<Vec<FleetRow>> {
    rows.iter().map(|row| admit(row, peer, ledger).map_err(|refusal| NorthError::Refused(refusal.why().to_owned()))).collect()
}

pub fn admit_exchange(exchange: Exchange, peer: &str, ledger: &[SessionRow], now_ms: u64) -> NorthResult<Exchange> {
    if exchange.host != peer {
        return Err(NorthError::Refused(format!("an exchange from {peer} names host {}", exchange.host)));
    }
    if let Some(session) = exchange.sessions.iter().find(|session| session.host != peer) {
        return Err(NorthError::Refused(format!("an exchange from {peer} registers {} on {}", session.name, session.host)));
    }
    let rows = admit_all(&exchange.rows, peer, ledger)?;
    let sessions: Vec<SessionRow> = exchange.sessions.into_iter().filter(|session| ledger.iter().all(|held| held.name != session.name || held.host == peer)).map(|session| stamped(session, ledger, now_ms)).collect();
    let seen: Vec<SessionRow> = ledger.iter().cloned().chain(sessions.iter().filter(|session| ledger.iter().all(|held| held.name != session.name)).cloned()).collect();
    let addressees = exchange.addressees.into_iter().filter(|addressee| on_peer(&seen, peer, &addressee.name).is_ok()).map(|addressee| floored(addressee, &seen, now_ms)).collect();
    let gates = exchange.gates.into_iter().filter(|gate| gate.host == peer && on_peer(ledger, peer, &gate.request.session).is_ok()).collect();
    Ok(Exchange { host: exchange.host, rows, sessions, addressees, gates, outranks: Vec::new() })
}

pub fn seen_by(exchanged: Exchanged, peer: &str) -> Exchanged {
    let ledger = exchanged.sessions;
    let gates = exchanged.gates.into_iter().filter(|gate| on_peer(&ledger, peer, &gate.request.session).is_ok()).collect();
    let waits = exchanged.waits.into_iter().filter(|wait| on_peer(&ledger, peer, &wait.session).is_ok()).collect();
    let rows = exchanged.rows.into_iter().filter(|row| on_peer(&ledger, peer, &row.to).is_ok()).collect();
    let sessions = ledger.iter().map(|session| if runs_on(session, peer) { session.clone() } else { SessionRow { name: session.name.clone(), host: session.host.clone(), status: session.status.clone(), ..SessionRow::default() } }).collect();
    Exchanged { sessions, gates, waits, rows, ..exchanged }
}

pub fn admit_registration(registration: &Registration, peer: &str, ledger: &[SessionRow], now_ms: u64) -> NorthResult<Registration> {
    let row = &registration.row;
    if row.host != peer {
        return Err(NorthError::Refused(format!("{peer} may not register {} on {}", row.name, row.host)));
    }
    if !may_hold(ledger, peer, &row.name) {
        return Err(NorthError::Refused(format!("{peer} may not register or take over {}: another host holds it, or it is a new name that does not start with {peer}-", row.name)));
    }
    Ok(Registration { row: SessionRow { created: iso(now_ms), ..row.clone() }, claim: registration.claim })
}

pub fn admit_wait_sessions<'a>(sessions: impl IntoIterator<Item = &'a str>, peer: &str, ledger: &[SessionRow]) -> NorthResult<()> {
    for session in sessions {
        on_peer(ledger, peer, session).map_err(|_| NorthError::Refused(format!("{peer} may not wait on or settle a wait of {session}, which is not a session on {peer}")))?;
    }
    Ok(())
}

pub fn admit_settle(keys: &[WaitKey], peer: &str, ledger: &[SessionRow]) -> NorthResult<()> {
    admit_wait_sessions(keys.iter().map(|key| key.session.as_str()), peer, ledger)
}
