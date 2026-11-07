use super::*;

struct Filer {
    worker: Option<String>,
    claimed: Vec<(Ref, String)>,
    files_its_item: bool,
}

impl Filed for Filer {
    fn connect(&mut self, _: &Ref, _: &Ref) -> Option<()> {
        Some(())
    }

    fn tidy(&mut self, _: &Ref) -> Result<bool, String> {
        Ok(false)
    }

    fn worker(&mut self) -> Option<String> {
        self.worker.clone()
    }

    fn claim(&mut self, new: &Ref, by: &str) -> Result<String, String> {
        self.claimed.push((new.clone(), by.to_owned()));
        Ok(format!("owns {}#{}", new.0, new.1))
    }

    fn files_its_item(&mut self) -> bool {
        self.files_its_item
    }
}

fn filed(number: u64, body: &str) -> HookInput {
    let command = format!("gh issue create -R tompassarelli/north --title 'Filed by its own worker' --body '{body}'");
    let payload = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "tool_response": {"stdout": format!("https://github.com/tompassarelli/north/issues/{number}\n")},
    });
    HookInput::parse(payload.to_string().as_bytes()).unwrap()
}

#[test]
fn an_issue_a_fleet_worker_files_is_claimed_for_it_and_a_blocker_or_a_proxy_filing_is_not() {
    let replay = [
        (736, "n-remote-guard", "Category: tooling\n\n## Done when\n- [ ] remote-serve refuses an ssh command outside its allowlist"),
        (737, "n-grant-order", "Defect: api_pool::open draws a later grant first\nCategory: bug-known-cause\nClass: ranking\n\n## Done when\n- [ ] the grant with credit left that expires first is drawn first"),
        (738, "n-record-mirror", "Category: tooling\n\n## Done when\n- [ ] the fleet record mirror reaches nexus"),
        (739, "n-firn-follow", "Category: tooling\n\n## Done when\n- [ ] the follow timer ships each landing to every host"),
    ];
    for (number, session, body) in replay {
        let mut filer = Filer { worker: Some(session.to_owned()), claimed: Vec::new(), files_its_item: false };
        let verdict = link_with(&filed(number, body), &mut filer);
        assert_eq!(filer.claimed, [(("tompassarelli/north".to_owned(), number), session.to_owned())], "north#{number} was staffed beside {session}, which filed it");
        assert!(matches!(verdict, Verdict::Advise(note) if note.contains("not staff it again")), "north#{number}");
        let mut blocker = Filer { worker: Some(session.to_owned()), claimed: Vec::new(), files_its_item: false };
        link_with(&filed(number + 100, &format!("Blocks: north#{number}\n{body}")), &mut blocker);
        assert!(blocker.claimed.is_empty(), "a blocker {session} files stays for dispatch");
        let mut proxy = Filer { worker: None, claimed: Vec::new(), files_its_item: false };
        link_with(&filed(number, body), &mut proxy);
        assert!(proxy.claimed.is_empty(), "an issue the proxy files stays for dispatch");
    }
}

#[test]
fn an_issue_a_worker_files_as_its_own_item_is_claimed_even_when_it_names_what_it_blocks_north_769() {
    let body = "Category: debugging-unknown-cause\nClass: a session host exits mid-task\nDefect: the host of north-743-d0351 exited at 11:04 AM\nBlocks: north#761\n\n## Done when\n- [ ] the host survives a deploy";
    let mut filer = Filer { worker: Some("n-host-death".to_owned()), claimed: Vec::new(), files_its_item: true };
    link_with(&filed(769, body), &mut filer);
    assert_eq!(filer.claimed, [(("tompassarelli/north".to_owned(), 769), "n-host-death".to_owned())], "dispatch staffed north-769-d0416 beside n-host-death, which filed north#769 as its item");
    assert!(FILE_IT_FIRST.is_match("Item: new north issue, file it first (bug, priority:now): a fleet session host exits mid-task"));
}

#[test]
fn a_create_with_a_category_dispatch_cannot_read_is_refused_naming_the_valid_ones_north_787() {
    let write = GhWrite { creates_issue: true, ..GhWrite::default() };
    let harness = shape(&write, "Category: harness\n\n## Done when\n- [ ] the gate holds");
    assert_eq!(harness.len(), 1, "{harness:?}");
    assert!(harness[0].contains("`Category: harness` is no category") && harness[0].contains("bug-known-cause"), "{harness:?}");
    assert!(shape(&write, "Category: tooling\n\n## Done when\n- [ ] the gate holds").is_empty());
    let labelled = GhWrite { labels: vec!["category:harness".into()], ..write };
    assert_eq!(shape(&labelled, "## Done when\n- [ ] the gate holds").len(), 1);
}
