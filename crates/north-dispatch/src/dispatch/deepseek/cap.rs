//! The opencode slot cap (north#1020): DeepSeek workers run beside the Claude workers, not out of them, so the sessions
//! that fill this cap are opencode sessions alone and no Claude worker, however many, counts against it.
use super::PROVIDER;

/// The opencode sessions among `providers`, one provider name per running session.
pub fn running<'a>(providers: impl IntoIterator<Item = &'a str>) -> usize {
    providers.into_iter().filter(|provider| *provider == PROVIDER).count()
}

/// Whether another opencode worker may start with `running` opencode sessions up and `cap` allowed.
pub fn admit(running: usize, cap: usize) -> Result<(), String> {
    if running >= cap { Err(format!("{running} opencode workers run, the cap of {cap} in harness/opencode.toml [dispatch] slot_cap; it starts once one ends")) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_workers_have_their_own_slot_cap_beside_three_claude_workers() {
        let mut fleet = vec!["claude"; 3];
        assert_eq!(admit(running(fleet.iter().copied()), 2), Ok(()), "three Claude workers leave every opencode slot free");
        fleet.push("opencode");
        assert_eq!(admit(running(fleet.iter().copied()), 2), Ok(()));
        fleet.push("opencode");
        let refused = admit(running(fleet.iter().copied()), 2).unwrap_err();
        assert!(refused.contains("cap of 2"), "{refused}");
        fleet.truncate(3);
        assert_eq!(running(fleet.iter().copied()), 0);
    }
}
