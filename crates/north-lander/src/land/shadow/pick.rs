use std::collections::HashMap;
use std::path::PathBuf;

use super::super::queue::{self, Ticket};

pub struct Work<T> {
    pub ticket: Ticket,
    pub found: T,
}

pub fn ordered(tickets: Vec<(PathBuf, Ticket)>, host: &str) -> Vec<(PathBuf, Ticket)> {
    let mut newest: HashMap<(PathBuf, String), u128> = HashMap::new();
    for (path, ticket) in tickets.iter().filter(|(_, ticket)| ticket.host == host) {
        let entry = newest.entry((ticket.root.clone(), ticket.branch.clone())).or_insert(0);
        *entry = (*entry).max(queue::queued_micros(path));
    }
    let mut kept: Vec<_> = tickets.into_iter().filter(|(path, ticket)| ticket.verdict.is_none() && ticket.host == host && newest[&(ticket.root.clone(), ticket.branch.clone())] == queue::queued_micros(path)).collect();
    kept.sort_by_key(|(path, ticket)| (!ticket.goes_first(), queue::queued_micros(path), path.clone()));
    kept
}

pub fn first_with_work<T>(tickets: Vec<(PathBuf, Ticket)>, host: &str, mut missing: impl FnMut(&PathBuf, &Ticket) -> Option<T>) -> Option<Work<T>> {
    ordered(tickets, host).into_iter().find_map(|(path, ticket)| missing(&path, &ticket).map(|found| Work { ticket, found }))
}
