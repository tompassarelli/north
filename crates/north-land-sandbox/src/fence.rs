use std::path::{Path, PathBuf};

use landlock::{ABI, Access, AccessFs, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, path_beneath_rules};
use north_base::verdict_cache::path_set::{self, Entry, Kind};

const LIBC_ETC_FILES: &[&str] = &["/etc/ld-nix.so.preload", "/etc/nsswitch.conf", "/etc/host.conf", "/etc/hosts", "/etc/resolv.conf", "/etc/localtime"];

pub fn restrict(path_set_file: &Path, tree: &Path, scratch: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path_set_file).map_err(|error| format!("path set {}: {error}", path_set_file.display()))?;
    let entries = path_set::parse(&text).map_err(|error| format!("{}: {error}", path_set_file.display()))?;
    let all = AccessFs::from_all(ABI::V9);
    let read = AccessFs::ReadFile | AccessFs::ReadDir;
    let status = Ruleset::default()
        .handle_access(all)
        .and_then(landlock::Ruleset::create)
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(tree_paths(tree, &entries, Kind::Content), AccessFs::ReadFile)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(tree_paths(tree, &entries, Kind::Listing), AccessFs::ReadDir)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(tree_paths(tree, &entries, Kind::Exec), AccessFs::Execute | AccessFs::ReadFile)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules([target_dir(tree), PathBuf::from("/nix/store")], read | AccessFs::Execute)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(["/dev/null", "/dev/zero"], AccessFs::ReadFile | AccessFs::WriteFile | AccessFs::Truncate)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(["/dev/urandom"], AccessFs::ReadFile)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(["/proc"], read)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(LIBC_ETC_FILES, AccessFs::ReadFile)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules([scratch], all)))
        .and_then(landlock::RulesetCreated::restrict_self)
        .map_err(|error| format!("Landlock: {error}"))?;
    if status.ruleset == RulesetStatus::NotEnforced {
        return Err(format!("this kernel does not enforce Landlock ({:?}), so --enforce would not restrict anything; refusing to run the command", status.landlock));
    }
    Ok(())
}

fn tree_paths<'a>(tree: &'a Path, entries: &'a [Entry], kind: Kind) -> impl Iterator<Item = PathBuf> + 'a {
    entries.iter().filter(move |entry| entry.kind == kind).map(|entry| tree.join(&entry.path))
}

fn target_dir(tree: &Path) -> PathBuf {
    tree.join(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()))
}
