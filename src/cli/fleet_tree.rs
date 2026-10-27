//! `north fleet tree`: this host's sessions and those a relayed host reported, nested under their parents.
use crate::error::NorthResult;
use crate::fleet::registry::Fleet;

struct Node {
    name: String,
    parent: Option<String>,
    line: String,
}

pub(super) fn tree(fleet: &Fleet) -> NorthResult<()> {
    let mut nodes: Vec<Node> = fleet
        .sessions()?
        .iter()
        .map(|record| Node { name: record.name.clone(), parent: record.parent.clone(), line: format!("{} [{}] {}/{}", record.name, fleet.status(record).label(), record.provider.label(), record.account) })
        .collect();
    for seen in fleet.remote_views() {
        let host = &seen.snapshot.host;
        nodes.extend(seen.snapshot.sessions.iter().map(|session| Node { name: session.name.clone(), parent: session.parent.clone(), line: format!("{} [{}] {}/{} on {host}", session.name, session.status, session.provider, session.account) }));
    }
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
