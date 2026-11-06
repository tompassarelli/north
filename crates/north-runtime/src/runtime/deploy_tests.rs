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
