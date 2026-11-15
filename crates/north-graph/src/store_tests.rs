use crate::store::Graph;

#[test]
fn a_current_store_opens_while_another_connection_holds_the_write_lock() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested/graph.db");
    Graph::open(&path).unwrap();
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    Graph::open_patiently(&path, std::time::Duration::from_millis(50)).unwrap();
}
