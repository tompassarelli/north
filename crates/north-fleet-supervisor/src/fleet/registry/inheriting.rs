use crate::error::NorthResult;
use crate::fleet::model::{LaunchRequest, Role, SessionRecord, Status};
use crate::fleet::records::{read_record, write_record};

use super::Fleet;

impl Fleet {
    /// A lead launched with `Follows:` naming a lead that is gone takes over that lead's live children on this host, so their reports and the lander's notices reach it, not the proxy their gone parent's messages fall back to. Returns the children it took.
    pub(super) fn inherit_workers(&self, request: &LaunchRequest, successor: &SessionRecord) -> NorthResult<Vec<String>> {
        if request.role() != Role::Lead {
            return Ok(Vec::new());
        }
        let mut briefs = std::iter::once(request.prompt.clone()).chain(request.named_brief());
        let Some(predecessor) = briefs.find_map(|text| followed(&text)) else { return Ok(Vec::new()) };
        if predecessor == successor.name || !self.is_gone(&predecessor)? {
            return Ok(Vec::new());
        }
        let mut taken = Vec::new();
        for child in self.sessions()?.into_iter().filter(|child| child.parent.as_deref() == Some(predecessor.as_str()) && self.status(child) != Status::Exited) {
            let session_dir = self.session_dir(&child.name)?;
            let mut record = read_record(&session_dir)?;
            record.parent = Some(successor.name.clone());
            write_record(&session_dir, &record)?;
            taken.push(child.name);
        }
        Ok(taken)
    }

    fn is_gone(&self, name: &str) -> NorthResult<bool> {
        if self.known_elsewhere(name)? {
            return Ok(false);
        }
        Ok(self.record(name).map_or(true, |record| self.status(&record) == Status::Exited))
    }
}

fn followed(brief: &str) -> Option<String> {
    let line = brief.lines().find_map(|line| line.trim_start().strip_prefix("Follows:"))?;
    line.split_whitespace().next().map(|word| word.trim_matches('`').to_owned())
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{fleet, live, live_under};
    use crate::fleet::model::{Delivery, LaunchRequest, ProviderName, Role, SessionRecord};
    use crate::fleet::prompt::PromptMode;

    fn successor(follows: &str, role: Role) -> (LaunchRequest, SessionRecord) {
        let request = LaunchRequest {
            verification: false,
            gates_aside: false,
            urgent: false,
            container_shell: false,
            container_dir: None,
            name: "lead2".into(),
            provider: ProviderName::new("claude"),
            account: "a".into(),
            cwd: "/tmp".into(),
            model: None,
            effort: None,
            prompt: format!("Follows: {follows} (retired)\n\nTake over north#882."),
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::North,
            idle_exit_minutes: None,
            parent: Some("proxy".into()),
            task_ref: None,
            claim_box: None,
            placement: None,
            machine: None,
            capacity_need: None,
            role: Some(role),
            resume: None,
            compact_window: None,
            route: None,
                    };
        let record = SessionRecord::launched(&request, None, None, false, 2);
        (request, record)
    }

    fn report_to_parent(fleet: &super::Fleet, worker: &str) {
        let parent = fleet.record(worker).unwrap().parent.unwrap();
        fleet.tell(&parent, &format!("[from {worker}] Done: landed."), Delivery::Queued);
    }

    #[test]
    fn a_follows_successor_to_a_retired_lead_receives_its_live_workers_next_reports_not_the_proxy() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let proxy = live(&fleet, "proxy");
        let lead2 = live(&fleet, "lead2");
        let _lead = live(&fleet, "lead");
        let _guards = live_under(&fleet, "guards", Some("lead"));
        fleet.retire("lead", "test", None).unwrap();

        report_to_parent(&fleet, "guards");
        assert_eq!(proxy.frames.lock().unwrap().len(), 1, "before the successor, the retired lead's worker reports to the proxy");

        let (request, record) = successor("lead", Role::Lead);
        assert_eq!(fleet.inherit_workers(&request, &record).unwrap(), ["guards"]);
        report_to_parent(&fleet, "guards");

        assert_eq!(fleet.record("guards").unwrap().parent.as_deref(), Some("lead2"));
        assert_eq!(proxy.frames.lock().unwrap().len(), 1, "the next report skips the proxy");
        assert!(lead2.frames.lock().unwrap().iter().any(|frame| frame.contains("Done: landed.")));
    }
}
