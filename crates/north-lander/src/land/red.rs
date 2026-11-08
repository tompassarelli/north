use std::path::PathBuf;

use super::queue::Ticket;

pub fn fixes(tickets: &[(PathBuf, Ticket)]) -> Vec<(PathBuf, Ticket)> {
    tickets.iter().filter(|(_, ticket)| ticket.red_fix.is_some()).cloned().collect()
}
