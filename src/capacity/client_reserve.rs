use crate::capacity::native_hold::LeaseFact;

/// The client slots admitted native leases hold for clients that haven't started, and the part of them a request from inside a holding lease draws on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClientHold {
    pub(crate) held: u32,
    pub(crate) drawn: u32,
}

/// Each native lease holds its `newClients` less the clients it has started (the most seen running in its scope, or those of the leases started inside it, counted once per owner so a relaunched client doesn't take its sibling's slot), for `window` milliseconds after admission.
pub(crate) fn client_hold(leases: &[LeaseFact], requester: Option<&str>, requested: u32, now: f64, window: f64) -> ClientHold {
    leases
        .iter()
        .filter(|lease| lease.class == "native" && lease.new_clients > 0 && now - lease.created_at < window)
        .fold(ClientHold::default(), |total, lease| {
            let started = lease.clients_seen.max(lease.running_clients).max(lease.started_owners.values().sum());
            let held = lease.new_clients.saturating_sub(started);
            let drawn = if requester == Some(lease.id.as_str()) { held.min(requested) } else { 0 };
            ClientHold { held: total.held + held, drawn: total.drawn + drawn }
        })
}
