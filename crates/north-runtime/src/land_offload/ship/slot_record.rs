//! `north capacity offload slot record NAME`: records `NAME` as a build slot of the checkout it runs in, in the directory the sandbox broker reads to tell which project a request asked from a slot belongs to (worker_sandbox/request_checkout.rs). The offload's script writes that record itself wherever it can; run inside a worker's domain, which keeps the records unwritable, it asks this verb, which the outside broker runs in the checkout it placed the request in. So the project is the broker's to name, never a word of the request, and the name is taken only when it is a slot name of that project.

use std::path::{Path, PathBuf};

use north_base::foundation::offload;

use super::{SlotClass, checkout, fail};

/// Whether `name` is a slot of the checkout whose slots are named `base`: a slot class's own name, or that name with a slot number.
fn slot_of(base: &str, name: &str) -> bool {
    [SlotClass::LandCheck, SlotClass::Background].into_iter().any(|class| {
        let first = class.slot_name(base);
        name == first || name.strip_prefix(&first).and_then(|rest| rest.strip_prefix('-')).is_some_and(|number| !number.is_empty() && number.bytes().all(|digit| digit.is_ascii_digit()))
    })
}

/// Records `name` as a slot of the checkout this process runs in; its exit code.
pub(in crate::land_offload) fn run(name: &str) -> u8 {
    let recorded = checkout(Path::new(".")).and_then(|(relative, base)| {
        if !slot_of(&base, name) {
            return Err(format!("{name} is no build slot of {relative}, so it is not recorded as one"));
        }
        let records = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is unset")?.join(offload::SLOT_RECORDS);
        std::fs::create_dir_all(&records).and_then(|()| std::fs::write(records.join(name), format!("{relative}\n"))).map_err(|error| format!("{}: {error}", records.join(name).display()))
    });
    recorded.map_or_else(|why| fail(&why), |()| 0)
}
