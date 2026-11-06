use super::habits::findings;
use super::rust_text::RustFile;
use super::test_determinism::check;
use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn the_north_tree_keeps_its_rust_habits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let found: Vec<String> = findings(&root, None).unwrap().iter().filter(|finding| !finding.advisory).map(|finding| finding.render()).collect();
    assert!(found.is_empty(), "{}", found.join("\n"));

    let files = |text: &str| vec![RustFile::new("crates/x/src/a.rs", text)];
    let planted = "fn real() { std::thread::sleep(d); }\n#[cfg(test)]\nmod tests {\n    fn waits() { std::thread::sleep(d); let t = SystemTime::now(); let free = statvfs(p); }\n}\n";
    let refused = check(&files(planted), &BTreeMap::new(), true);
    assert_eq!(refused.iter().map(|finding| finding.line).collect::<Vec<_>>(), [4, 4, 4]);
    assert!(refused[0].text.contains("start_paused") && refused[2].text.contains("AGENT_CAPACITY_DISK_FREE_GIB"));
    let paused = "#[cfg(test)]\nmod tests {\n    #[tokio::test(start_paused = true)]\n    async fn waits() { tokio::time::sleep(d).await; }\n}\n";
    assert!(check(&files(paused), &BTreeMap::new(), true).is_empty());
    let at = |count: usize| BTreeMap::from([("crates/x/src/a.rs".to_owned(), count)]);
    let one = "#[cfg(test)]\nmod tests {\n    fn a() { std::thread::sleep(d); }\n}\n";
    assert!(check(&files(one), &at(1), true).is_empty());
    assert_eq!(check(&files(one), &at(0), true).len(), 1);
    assert!(check(&files(one), &at(2), true)[0].text.contains("lower the entry"));
    assert!(check(&files(one), &at(2), false).is_empty());
}
