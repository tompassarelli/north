use crate::{Config, Finding, Shape, Source, scan};

const SPEND: &str = r#"
pub fn mark_spent(fleet: &Fleet, org: &Org, record: &Record, home: &Path) {
    let marks = home.join(".local/state/north/api-exhausted");
    match std::fs::create_dir_all(marks).and_then(|()| std::fs::write(marks.join(&org.id), format!("{} {}\n", record.name, today()))) {
        Ok(()) => {}
        Err(error) => eprintln!("{error}"),
    }
}
"#;

const CREDIT: &str = r#"
fn sources(state: &Path, id: String) -> bool {
    let exhausted = state.join("north/api-exhausted").join(&id).exists();
    exhausted
}
"#;

fn found(files: &[Source], context: &[Source]) -> Vec<(Shape, usize, String)> {
    let mut out: Vec<(Shape, usize, String)> = scan(&Config::default(), files, context).expect("scan").into_iter().map(|Finding { shape, line, item, .. }| (shape, line, item)).collect();
    out.sort();
    out
}

fn source<'a>(path: &'a str, text: &'a str) -> Source<'a> {
    Source { path, text }
}

#[test]
fn replays_the_api_exhausted_marks_of_2026_11_07() {
    let writer = source("api_pool.rs", SPEND);
    let found = found(&[source("crates/north-ledger/src/credit.rs", CREDIT)], &[writer]);
    assert_eq!(found, [(Shape::PresenceFlag, 3, "api-exhausted/*".to_owned())]);
    let unwritten = self::found(&[source("crates/north-ledger/src/credit.rs", CREDIT)], &[]);
    assert_eq!(unwritten, []);
}
