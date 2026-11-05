use super::*;
use compose::{HOOK_LIMIT_CHARS, Shape, indexed};
use store::{SCHEMA, Sqlite};

#[derive(Deserialize)]
struct Labelled {
    id: String,
    question: String,
    needed: Vec<Needed>,
}

#[derive(Deserialize)]
struct Needed {
    #[serde(rename = "ref")]
    reference: String,
}

fn found(block: &str, row: &Labelled) -> usize {
    row.needed.iter().filter(|n| block.contains(&n.reference)).count()
}

#[test]
fn the_capped_grounding_block_recalls_what_the_pre_cap_block_did() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let south = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut db = Sqlite::open(&state.path().join("index.db"), false).unwrap();
    let mut sql = format!("{SCHEMA}\nBEGIN;\n");
    let (checkout, label) = (display(&root), format!("{}:", root.parent().unwrap().file_name().unwrap().to_string_lossy()));
    for mut doc in sources::repo_docs(root, south.path().to_owned()) {
        doc.reference = doc.reference.replacen(&checkout, "~/code/north/main", 1);
        doc.title = doc.title.replacen(&label, "north:", 1);
        sql.push_str(&sources::insert(&doc));
    }
    sql.push_str("COMMIT;");
    db.exec(&sql).unwrap();
    let rows: Vec<Labelled> = include_str!("fixtures/recall.jsonl").lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert!(rows.len() >= 20);
    let now = epoch("2026-11-05T12:00:00Z");
    let (mut needed, mut before, mut after, mut widest) = (0, 0, 0, 0);
    for row in &rows {
        let pre = indexed(&row.question, &mut db, now, &Shape::query()).unwrap().text;
        let capped = indexed(&row.question, &mut db, now, &Shape::hook(Default::default(), None)).unwrap().text;
        println!("{} pre-cap {}/{} capped {}/{} chars {}", row.id, found(&pre, row), row.needed.len(), found(&capped, row), row.needed.len(), ulen(&capped));
        needed += row.needed.len();
        before += found(&pre, row);
        after += found(&capped, row);
        widest = widest.max(ulen(&capped));
    }
    println!("recall: pre-cap {before}/{needed}, capped {after}/{needed}, widest capped block {widest} chars");
    assert!(widest <= HOOK_LIMIT_CHARS);
    assert!(after >= before, "capped recall {after}/{needed} is below the pre-cap block's {before}/{needed}");
}
