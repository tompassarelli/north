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

#[test]
fn an_accept_recorded_on_the_reviewers_host_counts_on_a_build_host_before_the_reviewer_is_told() {
    use crate::platform::review_accept::{Accept, Accepts};
    let patch = "0123456789abcdef0123456789abcdef01234567";
    let (laptop, build) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let stamp = Stamp { host: "whiterabbit".into(), state: laptop.path().to_owned(), at: 1.0 };
    fs::create_dir_all(build.path().join(ROOT).join("north")).unwrap();
    fs::write(build.path().join(ROOT).join(STAMP), serde_json::to_vec(&stamp).unwrap()).unwrap();
    let store = Accepts(laptop.path().join(ACCEPTS));
    let target = format!("{}/", build.path().join(ROOT).join(ACCEPTS).display());
    let counted = || Accepts::counted(&records_mirror::Records::of(build.path(), "gex45")).sessions(patch).unwrap();
    for session in ["commit-reviewer", "second-reviewer"] {
        store.record(&Accept { patch_id: patch.into(), session: session.into(), at: 1 }).unwrap();
        assert_eq!(carry(&store.0, std::slice::from_ref(&target)), Vec::<String>::new());
    }
    assert_eq!(counted().len(), 2, "a pre-queue scan on gex45 counts both accepts recorded on whiterabbit");
    store.withdraw(patch, "second-reviewer").unwrap();
    carry(&store.0, std::slice::from_ref(&target));
    assert_eq!(counted().into_iter().collect::<Vec<_>>(), ["commit-reviewer"], "a withdrawn accept stops counting there too");
}
