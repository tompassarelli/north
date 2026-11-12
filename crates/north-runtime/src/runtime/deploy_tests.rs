use super::super::failure::failed;
use super::super::layout;
use super::{BUILT_FROM, TREE, install_script, not_moved_back};
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
    fs::write(directory.join(BUILT_FROM), "stamped\n").unwrap();
    if with_tree {
        fs::create_dir_all(directory.join(TREE)).unwrap();
    }
}

fn install(home: &Path) {
    let status = Command::new("sh").arg("-c").arg(install_script(TARGET)).current_dir(home).status().unwrap();
    assert!(status.success());
}

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

#[test]
fn a_shipped_install_waits_for_whoever_is_writing_the_runtime_directory() {
    use rustix::fs::{FlockOperation, flock};
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let live = runtimes(home).join(TARGET);
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("north"), "half made\n").unwrap();
    staged(home, true);
    let writing = fs::File::create(runtimes(home).join(format!("{TARGET}.lock"))).unwrap();
    flock(&writing, FlockOperation::LockExclusive).unwrap();
    let shims = home.join("shims");
    fs::create_dir_all(&shims).unwrap();
    write_executable(&shims.join("flock"), format!("#!/bin/sh\ntouch asked\nPATH='{}' exec flock \"$@\"\n", std::env::var("PATH").unwrap()));
    let mut install = Command::new("sh").arg("-c").arg(install_script(TARGET)).env("PATH", format!("{}:{}", shims.display(), std::env::var("PATH").unwrap())).current_dir(home).spawn().unwrap();
    assert!(north_base::platform::wait::until(std::time::Duration::from_secs(10), || home.join("asked").exists()), "the install never asked for the runtime directory's lock");
    assert!(install.try_wait().unwrap().is_none(), "the install went ahead while the directory was being written");
    assert_eq!(fs::read_to_string(live.join("north")).unwrap(), "half made\n");
    drop(writing);
    assert!(install.wait().unwrap().success());
    assert!(live.join(TREE).is_dir() && fs::read_to_string(live.join("north")).unwrap() == "staged\n");
}

fn whole(home: &Path, sha: &str) {
    let directory = runtimes(home).join(sha);
    fs::create_dir_all(directory.join(TREE)).unwrap();
    for program in super::PROGRAMS {
        write_executable(&directory.join(program), "built\n");
    }
    fs::write(directory.join(BUILT_FROM), "stamped\n").unwrap();
}

fn prepared(home: &Path) -> super::Prepared {
    let output = Command::new("sh").arg("-c").arg(super::prepare_script(TARGET, &std::collections::BTreeSet::new())).current_dir(home).output().unwrap();
    assert!(output.status.success());
    super::Prepared::read(&String::from_utf8_lossy(&output.stdout))
}

#[test]
fn a_runtime_already_whole_on_the_host_is_not_copied_and_a_missing_one_is_copied_against_the_newest_whole_one() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let older = "0c54814b5c34a736fc44672f7e752782a7dbcd03";
    whole(home, older);
    fs::create_dir_all(runtimes(home).join("c3f70f1043954eabcea7d6f4c5043b1684c64fe9.staging")).unwrap();
    assert_eq!(prepared(home), super::Prepared::Staging(Some(older.to_owned())));
    assert!(runtimes(home).join(format!("{TARGET}.staging")).is_dir());
    fs::File::open(runtimes(home).join(older)).unwrap().set_modified(std::time::UNIX_EPOCH).unwrap();
    let newer = "c3f70f1043954eabcea7d6f4c5043b1684c64fe9";
    whole(home, newer);
    assert_eq!(prepared(home), super::Prepared::Staging(Some(newer.to_owned())), "with no current link the newest whole runtime is the basis");
    std::os::unix::fs::symlink(super::layout::link(older), home.join(".local/state/north/current")).unwrap();
    assert_eq!(prepared(home), super::Prepared::Staging(Some(older.to_owned())), "the live runtime is the basis the binary diff is taken from");
    whole(home, TARGET);
    assert_eq!(prepared(home), super::Prepared::Whole);
}

#[test]
fn a_runtime_on_the_host_without_its_built_from_record_is_not_whole() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    whole(home, TARGET);
    fs::remove_file(runtimes(home).join(TARGET).join(BUILT_FROM)).unwrap();
    assert_eq!(prepared(home), super::Prepared::Staging(None));
}

#[test]
fn a_runtime_replaced_while_it_waited_its_turn_is_not_shipped() {
    let state = tempfile::tempdir().unwrap();
    let layout = super::Layout { state: state.path().to_owned() };
    let references = std::collections::BTreeSet::new();
    let skipped = super::shipped(&layout, "no-such-host.invalid", TARGET, state.path(), &references, &|| true);
    assert!(skipped.is_ok());
}

#[test]
fn an_order_that_cannot_move_a_host_back_names_the_host_and_its_runtime_from_before_the_move_back() {
    let unknown = not_moved_back("builder", TARGET, failed(64, "north runtime update --sha --ordered on builder failed: exit status: 64"));
    let said = format!("builder cannot be moved back to the ordered {}: the runtime live on builder is from before an order could move a host back", layout::short(TARGET));
    assert!(unknown.code == 64 && unknown.message.starts_with(&said), "{}", unknown.message);
    let held = not_moved_back("builder", TARGET, failed(1, "did not go live here"));
    assert_eq!((held.code, held.message.as_str()), (1, "did not go live here"), "a host whose runtime takes the flag keeps its own reason");
}
