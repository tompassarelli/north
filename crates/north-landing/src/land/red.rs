use std::path::PathBuf;

use super::queue::Ticket;

pub fn fixes(tickets: &[(PathBuf, Ticket)]) -> Vec<(PathBuf, Ticket)> {
    tickets.iter().filter(|(_, ticket)| ticket.red_fix.is_some()).cloned().collect()
}

pub fn narrows(included: &[(&PathBuf, &Ticket)]) -> bool {
    let fixes = included.iter().filter(|(_, ticket)| ticket.red_fix.is_some()).count();
    fixes > 0 && fixes < included.len()
}

pub fn held(detail: &str, lanes: usize) -> String {
    format!("red-main {detail}: holding {lanes} lanes until a red-fix lane lands")
}
