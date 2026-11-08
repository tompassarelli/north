use std::collections::BTreeMap;

use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, SeccompRule, apply_filter};

use super::arch;

const TRACE_TAG: u32 = 1;

pub fn install() -> Result<(), String> {
    apply(arch::NUMBERS.iter().map(|(_, number)| *number), SeccompAction::Trace(TRACE_TAG))?;
    apply([arch::IO_URING_SETUP], SeccompAction::Errno(libc::ENOSYS as u32))
}

fn apply(numbers: impl IntoIterator<Item = i64>, action: SeccompAction) -> Result<(), String> {
    let failed = |error: &dyn std::fmt::Display| format!("seccomp: {error}");
    let rules: BTreeMap<i64, Vec<SeccompRule>> = numbers.into_iter().map(|number| (number, Vec::new())).collect();
    let filter = SeccompFilter::new(rules, SeccompAction::Allow, action, arch::TARGET).map_err(|error| failed(&error))?;
    let program: BpfProgram = filter.try_into().map_err(|error| failed(&error))?;
    apply_filter(&program).map_err(|error| failed(&error))
}
