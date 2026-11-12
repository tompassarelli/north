//! The laptop is no client of the build hosts' shared Buck2 store (north#1115 box 6a): a build started there goes whole to a
//! build host through `north capacity offload run`. This refuses a `buck2` command on that machine once anything Buck2
//! reads names a remote execution service, so pointing the laptop at the store stops at one line, not at a build.
use std::fs;
use std::path::{Path, PathBuf};

use north_base::platform::orchestration;

const SECTION: &str = "buck2_re_client";
const ADDRESSES: [&str; 3] = ["engine_address", "action_cache_address", "cas_address"];

/// The buckconfig files Buck2 reads for a project at `root`: the system's under `etc`, the user's under `home` and the project's, each with its `.d` directory, and the project's local file.
fn configs(root: &Path, etc: &Path, home: &Path) -> Vec<PathBuf> {
    let files = [etc.join("buckconfig"), home.join(".buckconfig"), root.join(".buckconfig"), root.join(".buckconfig.local")];
    let directories = [etc.join("buckconfig.d"), home.join(".buckconfig.d"), root.join(".buckconfig.d")];
    let mut nested: Vec<PathBuf> = directories.iter().flat_map(|directory| fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path())).collect();
    nested.sort();
    files.into_iter().chain(nested).collect()
}

/// Whether buckconfig `text` gives `[buck2_re_client]` the address of an execution service, an action cache or a store.
fn names_service(text: &str) -> bool {
    let mut section = "";
    text.lines().map(str::trim).any(|line| match line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        Some(name) => {
            section = name.trim();
            false
        }
        None => section == SECTION && line.split_once('=').is_some_and(|(key, value)| ADDRESSES.contains(&key.trim()) && !value.trim().is_empty()),
    })
}

fn runs_buck2(command: &[String]) -> bool {
    command.iter().any(|word| word.split(|letter: char| !letter.is_ascii_alphanumeric() && letter != '_').any(|part| part == "buck2"))
}

/// Why `command`, run for the project at `root`, does not start on the laptop (`laptop`): it runs `buck2` and its own words or a buckconfig name a remote execution service.
pub(super) fn refusal(laptop: bool, command: &[String], root: &Path, etc: &Path, home: &Path) -> Option<String> {
    if !laptop || !runs_buck2(command) {
        return None;
    }
    let on_command_line = command.iter().any(|word| word.contains("buck2_re_client.")).then(|| "the command line".to_owned());
    let named = on_command_line.or_else(|| configs(root, etc, home).into_iter().find(|file| fs::read_to_string(file).is_ok_and(|text| names_service(&text))).map(|file| file.display().to_string()))?;
    Some(format!(
        "north capacity dev-shell: {named} names a remote execution service, and the laptop is no client of the build hosts' Buck2 store (north#1115 box 6a); remove that setting and start the build on a build host with `north capacity offload run`"
    ))
}

/// [`refusal`] for this machine, which is the laptop when the orchestration policy says so.
pub(super) fn refusal_here(command: &[String], root: &Path) -> Option<String> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let laptop = runs_buck2(command) && orchestration::owners_machine(&orchestration::path()) == Some(super::local_machine::host_name());
    refusal(laptop, command, root, Path::new("/etc"), &home)
}

#[cfg(test)]
#[path = "store_client_tests.rs"]
mod tests;
