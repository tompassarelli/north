use toml_edit::{DocumentMut, Item};

#[cfg(test)]
mod tests;

pub(super) fn deploy(cfg: &DocumentMut, here: &str) -> Vec<String> {
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return Vec::new() };
    let mut destinations: Vec<String> = Vec::new();
    for (_, host) in hosts.iter().filter(|(name, _)| *name != here) {
        let destination = ["deploy_ssh", "ssh"].iter().find_map(|key| host.get(key)?.as_str());
        if let Some(destination) = destination.filter(|destination| !destinations.iter().any(|seen| seen == destination)) {
            destinations.push(destination.to_owned());
        }
    }
    destinations
}

pub(super) fn builds_runtime(cfg: &DocumentMut, here: &str) -> bool {
    cfg.get("hosts")
        .and_then(Item::as_table_like)
        .and_then(|hosts| hosts.get(here))
        .and_then(|host| host.get("builds_runtime"))
        .and_then(Item::as_bool)
        .unwrap_or(true)
}
