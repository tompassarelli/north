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

struct Repo;

impl World for Repo {
    fn repo(&mut self, named: Option<&str>) -> Option<String> {
        named.map(str::to_owned)
    }

    fn claimed(&mut self) -> Vec<String> {
        Vec::new()
    }

    fn open(&mut self, _: &str) -> Vec<(u64, String)> {
        Vec::new()
    }

    fn worker_brief(&mut self) -> Option<String> {
        None
    }

    fn children_filed(&mut self) -> usize {
        0
    }
}

struct Worker {
    filed: usize,
}

impl World for Worker {
    fn repo(&mut self, named: Option<&str>) -> Option<String> {
        named.map(str::to_owned)
    }

    fn claimed(&mut self) -> Vec<String> {
        vec!["north#1137".to_owned()]
    }

    fn open(&mut self, _: &str) -> Vec<(u64, String)> {
        Vec::new()
    }

    fn worker_brief(&mut self) -> Option<String> {
        Some("Item: tompassarelli/north#1137\nDone when: `cargo nextest run -p north-guard` exits 0".to_owned())
    }

    fn children_filed(&mut self) -> usize {
        self.filed
    }
}

fn creating(command: &str) -> HookInput {
    let payload = serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": command}, "cwd": "/"});
    HookInput::parse(payload.to_string().as_bytes()).unwrap()
}

#[test]
fn a_body_file_the_command_builds_from_a_file_with_boxes_is_judged_by_both_n867() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("body.md"), "Category: tooling\n\n## Done when\n\n- [ ] the revert guard passes a moved import\n").unwrap();
    std::fs::write(dir.path().join("note.md"), "Category: tooling\n\nNo checks yet.\n").unwrap();
    let built = |source: &str| {
        let dir = dir.path().display();
        format!("cd {dir} && {{ printf 'Class: revert guard false positive on a moved import line\\n'; cat {source}; }} > body2.md && cd / && gh issue create -R tompassarelli/north --label bug --title 'Revert guard flags a moved import' --body-file {dir}/body2.md 2>&1 | tail -n 2")
    };
    assert_eq!(decide_with(&creating(&built("body.md")), &mut Repo), Verdict::Allow);
    assert!(matches!(decide_with(&creating(&built("note.md")), &mut Repo), Verdict::Deny(reason) if reason.contains("no `- [ ]` box")));
    let rewritten = format!("cd {} && {{ printf 'Class: revert guard false positive\\n'; sed '/^Done when:$/,$d' note.md; printf '## Done when\\n\\n- [ ] the revert guard passes a moved import\\n'; }} > body3.md && gh issue create -R tompassarelli/north --label bug --title 'Revert guard flags a moved import' --body-file {}/body3.md", dir.path().display(), dir.path().display());
    assert_eq!(decide_with(&creating(&rewritten), &mut Repo), Verdict::Allow);
}

#[test]
fn a_body_file_named_by_a_variable_the_command_assigns_is_judged_by_its_boxes_north_1041() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("body.md"), "Category: tooling\n\n## Done when\n\n- [ ] the body file is read\n").unwrap();
    std::fs::write(dir.path().join("empty.md"), "Category: tooling\n\nNo checks yet.\n").unwrap();
    let path = dir.path().display();
    for command in [
        format!("F={path}/body.md && gh issue create -R tompassarelli/north --label enhancement --title 'Read a body file' --body-file $F"),
        format!("F={path} && gh issue create -R tompassarelli/north --label enhancement --title 'Read a body file' --body-file ${{F}}/body.md"),
        format!("S={path} && gh issue create -R tompassarelli/north --label enhancement --title 'Read a body file' --body \"$(cat $S/body.md)\""),
    ] {
        assert_eq!(decide_with(&creating(&command), &mut Repo), Verdict::Allow, "{command}");
    }
    let missing = format!("F={path}/empty.md && gh issue create -R tompassarelli/north --label enhancement --title 'Check a body file' --body-file $F");
    assert!(matches!(decide_with(&creating(&missing), &mut Repo), Verdict::Deny(reason) if reason.contains("no `- [ ]` box")));
}

#[test]
fn a_worker_files_a_child_of_its_own_issue_and_is_refused_an_issue_with_no_parent() {
    let create = |parent: &str| {
        creating(&format!(
            "gh issue create -R tompassarelli/north --label enhancement --title 'Read the leaf row' --body 'Category: feature\n{parent}Done when: `cargo nextest run -p north-guard` exits 0\n'"
        ))
    };
    assert_eq!(decide_with(&create("Parent: north#1137\n"), &mut Worker { filed: 0 }), Verdict::Allow);
    assert!(matches!(decide_with(&create(""), &mut Worker { filed: 0 }), Verdict::Deny(_)));
    assert!(matches!(decide_with(&create("Parent: north#900\n"), &mut Worker { filed: 0 }), Verdict::Deny(reason) if reason.contains("do not hold")));
    let limit = north_base::platform::harness::default_number("worker_children_per_session") as usize;
    assert!(matches!(decide_with(&create("Parent: north#1137\n"), &mut Worker { filed: limit }), Verdict::Deny(reason) if reason.contains("at most")));
    let unrunnable = creating("gh issue create -R tompassarelli/north --label enhancement --title 'Read the leaf row' --body 'Category: feature\nParent: north#1137\nDone when: it works\n'");
    assert!(matches!(decide_with(&unrunnable, &mut Worker { filed: 0 }), Verdict::Deny(reason) if reason.contains("runnable")));
}
