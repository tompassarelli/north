use super::*;

fn entry(name: &str, size: u64) -> Entry {
    Entry {
        path: PathBuf::from(name),
        signature: (size, 1),
    }
}

#[test]
fn a_records_mirror_run_that_times_out_keeps_what_it_sent_and_the_next_run_sends_only_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let sent = dir.path().join("sent");
    let current = vec![
        entry("a", BATCH_BYTES),
        entry("b", BATCH_BYTES),
        entry("c", 1),
    ];
    let mut calls = 0;
    let cut = sync(&sent, &current, |_| {
        calls += 1;
        if calls == 1 {
            Ok(())
        } else {
            Err("timed out after 600s".to_owned())
        }
    });
    assert!(cut.is_err());
    let mut resent: Vec<Vec<PathBuf>> = Vec::new();
    sync(&sent, &current, |paths| {
        resent.push(paths.to_vec());
        Ok(())
    })
    .unwrap();
    let resent: Vec<PathBuf> = resent.concat();
    assert_eq!(resent, vec![PathBuf::from("b"), PathBuf::from("c")]);
}
