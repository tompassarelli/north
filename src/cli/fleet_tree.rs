//! `north fleet tree`: this host's sessions and those the fleet table holds for other hosts, nested under their parents.
use std::collections::BTreeSet;

use crate::error::NorthResult;
use crate::fleet::registry::Fleet;

use super::fleet_list::elsewhere;

struct Node {
    name: String,
    parent: Option<String>,
    line: String,
}

pub(super) fn tree(fleet: &Fleet) -> NorthResult<()> {
    let records = fleet.sessions()?;
    let named: BTreeSet<String> = records.iter().map(|record| record.name.clone()).collect();
    let mut nodes: Vec<Node> = records
        .iter()
        .map(|record| Node { name: record.name.clone(), parent: record.parent.clone(), line: format!("{} [{}] {}/{}", record.name, fleet.status(record).label(), record.provider.label(), record.account) })
        .collect();
    nodes.extend(elsewhere(fleet, &named).into_iter().map(|row| Node { line: format!("{} [{}] {}/{} on {}", row.name, row.status, row.provider, row.account, row.host), name: row.name, parent: row.parent }));
    let known = |name: &String| nodes.iter().any(|node| &node.name == name);
    for root in nodes.iter().filter(|node| !node.parent.as_ref().is_some_and(known)) {
        print_branch(&nodes, root, 0);
    }
    Ok(())
}

fn print_branch(nodes: &[Node], node: &Node, depth: usize) {
    println!("{}{}", "  ".repeat(depth), node.line);
    for child in nodes.iter().filter(|child| child.parent.as_deref() == Some(node.name.as_str())) {
        print_branch(nodes, child, depth + 1);
    }
}
