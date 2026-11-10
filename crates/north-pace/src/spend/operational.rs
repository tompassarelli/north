//! The reserve operational callers (the privacy judge, lander and canaries) draw on before shared
//! capacity (north#1049, north#1076): `[accounts.claude] reserve` in the orchestration policy names its
//! account labels, and an operational ask tries them first.
use toml_edit::{DocumentMut, Item};

/// The account labels `[accounts.claude] reserve` declares.
pub fn declared(cfg: &DocumentMut) -> Vec<String> {
    let listed = cfg.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("reserve")).and_then(Item::as_array);
    listed.into_iter().flatten().filter_map(|label| label.as_str().map(str::to_owned)).collect()
}

/// `candidates` in the order an operational caller tries them: the reserve's accounts first, then the
/// shared ones, each group keeping its own order.
pub fn order(reserve: &[String], candidates: Vec<String>) -> Vec<String> {
    let (held, shared): (Vec<String>, Vec<String>) = candidates.into_iter().partition(|label| reserve.contains(label));
    held.into_iter().chain(shared).collect()
}
