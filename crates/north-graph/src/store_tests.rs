use std::sync::{Arc, Barrier};

use crate::store::{Graph, KEYED_BY_VALUE, SCHEMA_VERSION};
use crate::term::{Term, Triple};

fn fact(writer: usize, index: usize) -> Triple {
    Triple::new(Term::string(format!("writer-{writer}")), Term::keyword("wrote"), i64::try_from(index).unwrap())
}

#[test]
fn a_current_store_opens_while_another_connection_holds_the_write_lock() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested/graph.db");
    Graph::open(&path).unwrap();
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    Graph::open_patiently(&path, std::time::Duration::from_millis(50)).unwrap();
}

#[test]
fn concurrent_writers_each_keep_every_acknowledged_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graph.db");
    Graph::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|writer| {
            let (path, barrier) = (path.clone(), Arc::clone(&barrier));
            std::thread::spawn(move || {
                let mut graph = Graph::open(&path).unwrap();
                barrier.wait();
                for index in 0..25 {
                    graph.tell(&format!("writer-{writer}"), fact(writer, index)).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let graph = Graph::open(&path).unwrap();
    let report = graph.validate().unwrap();
    assert_eq!(report.occurrences, 200);
    assert_eq!(report.live_triples, 200);
    assert!(report.is_sound(), "{:?}", report.problems);
    for writer in 0..8 {
        assert_eq!(graph.show(&Term::string(format!("writer-{writer}"))).unwrap().len(), 25);
    }
}

#[test]
fn an_old_state_store_opens_writable_and_read_only_and_keeps_its_facts() {
    for (old_state, reshape) in [
        (KEYED_BY_VALUE, "CREATE TABLE terms_v2 (id INTEGER PRIMARY KEY, key BLOB NOT NULL UNIQUE); INSERT INTO terms_v2 SELECT id, key FROM terms; DROP TABLE terms; ALTER TABLE terms_v2 RENAME TO terms;"),
        (SCHEMA_VERSION - 1, ""),
    ] {
        for read_only in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("graph.db");
            let mut graph = Graph::open(&path).unwrap();
            graph.tell("writer-0", fact(0, 1)).unwrap();
            let head = graph.head().unwrap();
            graph.connection().execute_batch(&format!("{reshape} PRAGMA user_version = {old_state};")).unwrap();
            drop(graph);
            let opened = if read_only { Graph::open_read_only(&path, std::time::Duration::from_secs(1)) } else { Graph::open(&path) };
            let opened = opened.unwrap_or_else(|why| panic!("{why}: schema {old_state}, read_only {read_only}"));
            assert_eq!(opened.head().unwrap(), head, "the upgraded store keeps every fact the older build told at schema {old_state}, read_only {read_only}");
        }
    }
}
