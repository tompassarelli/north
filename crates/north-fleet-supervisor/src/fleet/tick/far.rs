use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::NorthResult;

use super::{FarHost, OpenLedger, Pulls, Taken, Ticked};
use crate::fleet::model::SessionRecord;
use crate::fleet::peer_row::{self, Refusal};
use crate::fleet::registry::Fleet;
use crate::fleet::stage::each_by;
use crate::fleet::store::{Exchange, FarRegistry, FarSpools, FleetRow, FleetTable, HandedRow, LAUNCH_GRACE_MS, RowKind, SessionRow, Undecodable, UnknownKind};

pub(super) fn deliver_far(ledger: &OpenLedger, pulls: &Pulls, taken: &[Taken], known: Vec<SessionRow>, ticked: &mut Ticked, ((deadline, budget), now_ms): ((Instant, Duration), u64)) {
    let known = Arc::new(known);
    let runs = taken.iter().map(|taken| {
        let (ledger, spools, known, host) = (ledger.clone(), pulls.spools.clone(), known.clone(), taken.host.clone());
        move || -> NorthResult<Vec<HandedRow>> {
            let FarRegistry { host: registered, sessions, addressees, gates } = spools.registry(&host.ssh)?;
            if registered.is_empty() {
                return Ok(Vec::new());
            }
            let admitted = peer_row::admit_exchange(Exchange { rows: Vec::new(), host: registered, sessions, addressees, gates, outranks: Vec::new() }, &host.name, &known, None, now_ms)?;
            let exchanged = ledger()?.exchange(&admitted)?;
            let rows: Vec<FleetRow> = exchanged.rows.into_iter().filter(|row| !exchanged.held_elsewhere.contains(&row.to)).collect();
            if rows.is_empty() { Ok(Vec::new()) } else { spools.deliver(&host.ssh, &rows) }
        }
    });
    for (taken, delivered) in taken.iter().zip(each_by("deliver_far", runs, deadline)) {
        match delivered {
            Ok(Ok(handed)) => ticked.note(handed),
            Ok(Err(error)) => ticked.far_failed.push((taken.host.name.clone(), error.user_message())),
            Err(late) => ticked.put_off(&format!("deliver_far on {}", taken.host.name), &late, budget),
        }
    }
}

pub(super) fn take_far(fleet: &Fleet, records: &[SessionRecord], cached: Vec<SessionRow>, pulls: &Pulls, rows: &mut Vec<FleetRow>, ticked: &mut Ticked, (now_ms, budget): (u64, Duration)) -> Vec<Taken> {
    let hosts = far_hosts(fleet, records, pulls);
    let runs = hosts.iter().map(|host| {
        let (spools, ssh) = (pulls.spools.clone(), host.ssh.clone());
        move || spools.take(&ssh)
    });
    let answers = each_by("take_far", runs, Instant::now() + budget);
    let known = ledger_records(fleet, records, cached, now_ms);
    let archived: Vec<SessionRecord> = fleet.past_sessions();
    let owner = fleet.owners_machine();
    let mut taken = Vec::new();
    for (host, answer) in hosts.into_iter().zip(answers) {
        match answer {
            Ok(Ok(found)) => {
                let (mut ids, mut refused) = (Vec::new(), Vec::new());
                for UnknownKind { id, kind } in found.unknown_kind {
                    ticked.left.push((host.name.clone(), format!("{id}: kind {kind} is not known to this build")));
                }
                for Undecodable { id, why } in found.undecodable {
                    ticked.refused.push((host.name.clone(), format!("{id}: {why}")));
                    if FleetRow::check_id(&id).is_ok() {
                        refused.push(id);
                    }
                }
                for row in found.rows {
                    let admitted = row.validate().map_err(|error| Refusal::Spoofed(format!("{}: {}", row.id, error.user_message()))).and_then(|()| peer_row::admit(&row, &host.name, &known, owner.as_deref()));
                    match admitted {
                        Ok(admitted) => {
                            ids.push(admitted.id.clone());
                            rows.push(admitted);
                        }
                        Err(Refusal::Unregistered(_)) if retired_report(&row, &host.name, &archived) => {
                            ids.push(row.id.clone());
                            rows.push(FleetRow { origin: Some(host.name.clone()), urgent: false, ..row });
                        }
                        Err(Refusal::Unregistered(why)) if now_ms.abs_diff(row.sent_at) > LAUNCH_GRACE_MS => {
                            ticked.refused.push((host.name.clone(), format!("{why}, and the ledger has not listed it within {} minutes", LAUNCH_GRACE_MS / 60_000)));
                            refused.push(row.id);
                        }
                        Err(Refusal::Unregistered(why)) => ticked.unregistered.push((host.name.clone(), why)),
                        Err(Refusal::Spoofed(why)) => {
                            ticked.refused.push((host.name.clone(), why));
                            if FleetRow::check_id(&row.id).is_ok() {
                                refused.push(row.id);
                            }
                        }
                    }
                }
                taken.push(Taken { host, ids, refused });
            }
            Ok(Err(error)) => ticked.pull_failed.push((host.name.clone(), error.user_message())),
            Err(late) => ticked.put_off(&format!("take_far on {}", host.name), &late, budget),
        }
    }
    taken
}

fn ledger_records(fleet: &Fleet, records: &[SessionRecord], mut known: Vec<SessionRow>, now_ms: u64) -> Vec<SessionRow> {
    let unlisted: Vec<SessionRow> = records.iter().filter(|record| !known.iter().any(|row| row.name == record.name)).map(|record| fleet.table_row(record, now_ms)).collect();
    known.extend(unlisted);
    known
}

fn retired_report(row: &FleetRow, peer: &str, archived: &[SessionRecord]) -> bool {
    row.kind == RowKind::Message && row.sender.session().is_some_and(|sender| archived.iter().any(|record| record.name == sender && record.machine.as_ref().is_some_and(|machine| machine.name == peer)))
}

pub(super) fn drop_far(pulls: &Pulls, taken: &[Taken], ticked: &mut Ticked, (deadline, budget): (Instant, Duration)) {
    let held: Vec<&Taken> = taken.iter().filter(|taken| !taken.ids.is_empty() || !taken.refused.is_empty()).collect();
    let runs = held.iter().map(|taken| {
        let (spools, ssh, ids, refused) = (pulls.spools.clone(), taken.host.ssh.clone(), taken.ids.clone(), taken.refused.clone());
        move || {
            let dropped = if ids.is_empty() { Ok(()) } else { FarSpools::drop(&*spools, &ssh, &ids) };
            dropped.and_then(|()| if refused.is_empty() { Ok(()) } else { spools.set_aside(&ssh, &refused) })
        }
    });
    for (taken, answer) in held.iter().zip(each_by("drop_far", runs, deadline)) {
        match answer {
            Ok(Ok(())) => {}
            Ok(Err(error)) => ticked.drop_failed.push((taken.host.name.clone(), error.user_message())),
            Err(late) => ticked.put_off(&format!("drop_far on {}", taken.host.name), &late, budget),
        }
    }
}

pub(super) fn cache_far(pulls: &Pulls, taken: &[Taken], table: &FleetTable, ticked: &mut Ticked, (deadline, budget): (Instant, Duration)) {
    let table = Arc::new(table.clone());
    let runs = taken.iter().map(|taken| {
        let (spools, ssh, table) = (pulls.spools.clone(), taken.host.ssh.clone(), table.clone());
        move || spools.cache(&ssh, &table)
    });
    for (taken, answer) in taken.iter().zip(each_by("cache_far", runs, deadline)) {
        match answer {
            Ok(Ok(())) => {}
            Ok(Err(error)) => ticked.uncached.push((taken.host.name.clone(), error.user_message())),
            Err(late) => ticked.put_off(&format!("cache_far on {}", taken.host.name), &late, budget),
        }
    }
}

pub(super) fn far_hosts(fleet: &Fleet, records: &[SessionRecord], pulls: &Pulls) -> Vec<FarHost> {
    let placement = fleet.placed_hosts();
    let placed = records.iter().filter(|record| !record.ended()).filter_map(|record| record.machine.as_ref()).filter(|machine| placement.as_ref().is_none_or(|placement| placement.contains(&machine.name))).map(|machine| FarHost { name: machine.name.clone(), ssh: machine.ssh.clone() });
    let mut hosts: Vec<FarHost> = Vec::new();
    for host in placed.chain(pulls.always.iter().cloned()).filter(|host| host.name != fleet.host()) {
        if !hosts.iter().any(|seen| seen.name == host.name || seen.ssh == host.ssh) {
            hosts.push(host);
        }
    }
    hosts
}
