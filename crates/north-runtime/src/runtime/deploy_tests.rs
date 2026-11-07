use super::{TREE, install_script};
use crate::foundation::shim::write_executable;
use std::fs;
use std::path::Path;
use std::process::Command;

const TARGET: &str = "1613a7786406485376d98593c765ee31cebbf632";

fn runtimes(home: &Path) -> std::path::PathBuf {
    home.join(".local/state/north/runtimes")
}

fn staged(home: &Path, with_tree: bool) {
    let directory = runtimes(home).join(format!("{TARGET}.staging"));
    fs::create_dir_all(&directory).unwrap();
    write_executable(&directory.join("north"), "staged\n");
    if with_tree {
        fs::create_dir_all(directory.join(TREE)).unwrap();
    }
}

fn install(home: &Path) {
    let status = Command::new("sh").arg("-c").arg(install_script(TARGET)).current_dir(home).status().unwrap();
    assert!(status.success());
}

// KEEP(bug): on 2026-11-06 an offloaded land check ran a runtime staged on a build host with its programs but no tree and panicked in harness resolution, so the lander marked a clean landing red main (north#634)
#[test]
fn a_staged_runtime_replaces_a_treeless_one_whole_and_a_complete_one_stays() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let live = runtimes(home).join(TARGET);
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("north"), "treeless\n").unwrap();
    staged(home, true);
    install(home);
    assert!(live.join(TREE).is_dir() && fs::read_to_string(live.join("north")).unwrap() == "staged\n", "the treeless runtime was replaced by the staged one, tree and all");
    assert!(!runtimes(home).join(format!("{TARGET}.staging")).exists() && !runtimes(home).join(format!("{TARGET}.old")).exists());

    fs::write(live.join("marker"), "kept\n").unwrap();
    staged(home, true);
    install(home);
    assert!(live.join("marker").is_file(), "a complete runtime is left untouched");
    assert!(!runtimes(home).join(format!("{TARGET}.staging")).exists());
}

fn whole(home: &Path, sha: &str) {
    let directory = runtimes(home).join(sha);
    fs::create_dir_all(directory.join(TREE)).unwrap();
    for program in super::PROGRAMS {
        write_executable(&directory.join(program), "built\n");
    }
}

fn prepared(home: &Path) -> super::Prepared {
    let output = Command::new("sh").arg("-c").arg(super::prepare_script(TARGET, &std::collections::BTreeSet::new())).current_dir(home).output().unwrap();
    assert!(output.status.success());
    super::Prepared::read(&String::from_utf8_lossy(&output.stdout))
}

// KEEP(bug): on 2026-11-07 the laptop re-sent all 85 MB of runtime 0c54814b5c to gex45 twice in a minute, 18 and 25 s each over its 230 ms link, after gex45 had built and installed that runtime itself, and every first copy sent the whole runtime though the host kept the previous one (north#697)
#[test]
fn a_runtime_already_whole_on_the_host_is_not_copied_and_a_missing_one_is_copied_against_the_newest_whole_one() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let older = "0c54814b5c34a736fc44672f7e752782a7dbcd03";
    whole(home, older);
    fs::create_dir_all(runtimes(home).join("c3f70f1043954eabcea7d6f4c5043b1684c64fe9.staging")).unwrap();
    assert_eq!(prepared(home), super::Prepared::Staging(Some(older.to_owned())));
    assert!(runtimes(home).join(format!("{TARGET}.staging")).is_dir());
    whole(home, TARGET);
    assert_eq!(prepared(home), super::Prepared::Whole);
}

// KEEP(bug): on 2026-11-07 a copy of a runtime this host had already replaced waited its turn behind one global lock and was shipped anyway, holding every host's newer copy behind the slowest host (north#731)
#[test]
fn a_runtime_replaced_while_it_waited_its_turn_is_not_shipped() {
    let state = tempfile::tempdir().unwrap();
    let layout = super::Layout { state: state.path().to_owned() };
    let references = std::collections::BTreeSet::new();
    let skipped = super::shipped(&layout, "no-such-host.invalid", TARGET, state.path(), &references, &|| true);
    assert!(skipped.is_ok());
}
