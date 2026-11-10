pub(super) fn runnable(key: &str, done_when: &str) -> String {
    if done_when.starts_with('`') {
        return done_when.to_owned();
    }
    format!("`north ledger flow detect --dry-run | rg -qF ' {key}: '; test $? -eq 1` exits 0, once {done_when}")
}

#[cfg(test)]
mod tests {
    use super::runnable;

    #[test]
    fn a_finding_whose_condition_opens_with_prose_files_a_box_naming_a_backticked_done_when_command_north_872() {
        let line = format!("Done when: {}", runnable("code-health-review:north:6f03d49689", "the review slices have landed."));
        assert!(line.starts_with("Done when: `"), "{line}");
        assert!(line.contains("rg -qF ' code-health-review:north:6f03d49689: '"), "{line}");
        assert_eq!(runnable("k", "`true` exits 0."), "`true` exits 0.");
    }
}
