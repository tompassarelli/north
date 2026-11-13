use super::super::Gc;
use super::super::evidence::IssueState;
use super::super::state::Owned;
use super::Container;

impl Gc {
    pub(super) fn surface(&mut self, sweep: &mut Container, place: &str, branch: &str, age_h: i64, refs: &[String]) -> Result<String, String> {
        let owner = self.owner(&sweep.name)?;
        let issues = if refs.is_empty() { vec!["fault: names no issue".to_string()] } else { refs.iter().zip(self.issue_states(refs)).map(|(item, state)| format!("{item}={}", state.label())).collect() };
        sweep.owned.push(Owned { place: place.into(), branch: branch.into(), age_h, refs: refs.to_vec(), issues, owner: owner.clone() });
        Ok(owner)
    }

    pub(super) fn list_open(&mut self, issues: &[String], states: &[IssueState], place: &str) {
        for (issue, state) in issues.iter().zip(states) {
            if *state == IssueState::Open {
                self.kept_open.entry(issue.clone()).or_default().push(place.to_owned());
            }
        }
    }
}
