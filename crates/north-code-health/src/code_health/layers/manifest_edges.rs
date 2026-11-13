use serde::Serialize;
use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item, TableLike};

const KINDS: [(&str, &str); 3] = [("dependencies", "normal"), ("dev-dependencies", "dev"), ("build-dependencies", "build")];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(in crate::code_health) struct ManifestEdge {
    from: String,
    to: String,
    kind: &'static str,
    direction: &'static str,
}

struct Placed {
    layer: i64,
    family: Option<String>,
}

pub(in crate::code_health) fn manifest_edges<'a>(structure: &str, manifests: impl Iterator<Item = &'a str>) -> Vec<ManifestEdge> {
    let placed = placements(structure);
    let documents: Vec<DocumentMut> = manifests.filter_map(|text| text.parse().ok()).collect();
    let members: Vec<&str> = documents.iter().filter_map(package_name).collect();
    let mut edges: Vec<ManifestEdge> = documents.iter().flat_map(|document| edges_of(document, &placed, &members)).collect();
    edges.sort_by(|a, b| (&a.from, &a.to, a.kind).cmp(&(&b.from, &b.to, b.kind)));
    edges.dedup();
    edges
}

fn edges_of(document: &DocumentMut, placed: &BTreeMap<String, Placed>, members: &[&str]) -> Vec<ManifestEdge> {
    let Some(from) = package_name(document) else { return Vec::new() };
    let Some(source) = placed.get(from) else { return Vec::new() };
    dependency_tables(document)
        .into_iter()
        .flat_map(|(table, kind)| table.iter().map(move |(key, entry)| (kind, entry.get("package").and_then(Item::as_str).unwrap_or(key))))
        .filter(|(_, to)| members.contains(to))
        .filter_map(|(kind, to)| Some(ManifestEdge { from: from.to_owned(), to: to.to_owned(), kind, direction: direction(source, placed.get(to)?)? }))
        .collect()
}

fn direction(source: &Placed, target: &Placed) -> Option<&'static str> {
    if target.layer > source.layer {
        Some("up")
    } else if target.layer == source.layer && target.family != source.family {
        Some("sideways")
    } else {
        None
    }
}

fn package_name(document: &DocumentMut) -> Option<&str> {
    document.get("package")?.get("name")?.as_str()
}

fn dependency_tables(document: &DocumentMut) -> Vec<(&dyn TableLike, &'static str)> {
    let mut scopes: Vec<&dyn TableLike> = vec![document.as_table()];
    if let Some(targets) = document.get("target").and_then(Item::as_table_like) {
        scopes.extend(targets.iter().filter_map(|(_, scope)| scope.as_table_like()));
    }
    scopes
        .into_iter()
        .flat_map(|scope| KINDS.iter().filter_map(move |(name, kind)| scope.get(name).and_then(Item::as_table_like).map(|table| (table, *kind))))
        .collect()
}

fn placements(structure: &str) -> BTreeMap<String, Placed> {
    let Ok(document) = structure.parse::<DocumentMut>() else { return BTreeMap::new() };
    let Some(crates) = document.get("crate_dag").and_then(|dag| dag.get("crate")).and_then(Item::as_array_of_tables) else { return BTreeMap::new() };
    crates
        .iter()
        .filter_map(|table| {
            let name = table.get("name")?.as_str()?.to_owned();
            let layer = table.get("layer")?.as_integer()?;
            Some((name, Placed { layer, family: table.get("family").and_then(Item::as_str).map(str::to_owned) }))
        })
        .collect()
}
