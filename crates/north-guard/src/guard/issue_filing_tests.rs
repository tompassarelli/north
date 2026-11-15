use super::*;

struct Filer {
    worker: Option<String>,
    claimed: Vec<(Ref, String)>,
    files_its_item: bool,
    intakes: Vec<(Ref, Vec<String>)>,
    may_launch: bool,
}

impl Filer {
    fn new(worker: Option<&str>, files_its_item: bool) -> Self {
        Self { worker: worker.map(str::to_owned), claimed: Vec::new(), files_its_item, intakes: Vec::new(), may_launch: true }
    }
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

    fn intake(&mut self, new: &Ref, gaps: &[String]) -> Result<String, String> {
        self.intakes.push((new.clone(), gaps.to_vec()));
        Ok("intake started".to_owned())
    }

    fn may_launch(&mut self) -> bool {
        self.may_launch
    }
}

fn created_with(labels: &str, body: &str, number: u64) -> HookInput {
    let command = format!("gh issue create -R tompassarelli/north {labels} --title 'Filed by its own worker' --body '{body}'");
    let payload = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "tool_response": {"stdout": format!("https://github.com/tompassarelli/north/issues/{number}\n")},
    });
    HookInput::parse(payload.to_string().as_bytes()).unwrap()
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
fn an_issue_a_worker_files_as_its_own_item_is_claimed_even_when_it_names_what_it_blocks_north_769() {
    let body = "Category: debugging-unknown-cause\nClass: a session host exits mid-task\nDefect: the host of north-743-d0351 exited at 11:04 AM\nBlocks: north#761\n\n## Done when\n- [ ] the host survives a deploy";
    let mut filer = Filer::new(Some("n-host-death"), true);
    link_with(&filed(769, body), &mut filer);
    assert_eq!(filer.claimed, [(("tompassarelli/north".to_owned(), 769), "n-host-death".to_owned())], "dispatch staffed north-769-d0416 beside n-host-death, which filed north#769 as its item");
    assert!(FILE_IT_FIRST.is_match("Item: new north issue, file it first (bug, priority:now): a fleet session host exits mid-task"));
}

#[test]
fn a_create_missing_a_part_dispatch_needs_is_filed_and_its_gaps_name_the_valid_ones_for_the_haiku_pass_north_787_1632() {
    let write = GhWrite { creates_issue: true, ..GhWrite::default() };
    let harness = missing(&write, "Category: harness\n\n## Done when\n- [ ] the gate holds `true`");
    assert!(harness.iter().any(|gap| gap.contains("`Category: harness` is no category") && gap.contains("bug-known-cause")), "{harness:?}");
    assert!(!missing(&write, "Category: tooling\n\n## Done when\n- [ ] the gate holds `true`").iter().any(|gap| gap.contains("category")));
    let labelled = GhWrite { labels: vec!["category:harness".into()], ..write };
    assert!(missing(&labelled, "## Done when\n- [ ] the gate holds `true`").iter().any(|gap| gap.contains("is no category")));

    let mut bare = Filer::new(None, false);
    link_with(&created_with("", "Item: x\n\n## Done when\n\n- [ ] the gate passes\n", 1632), &mut bare);
    let [(new, gaps)] = &bare.intakes[..] else { panic!("one intake for the bare create: {:?}", bare.intakes) };
    assert_eq!(new, &("tompassarelli/north".to_owned(), 1632));
    assert!(gaps.iter().any(|gap| gap.starts_with("no type label")), "{gaps:?}");
    assert!(gaps.iter().any(|gap| gap.contains("`Category:` line")), "{gaps:?}");
    assert!(gaps.iter().any(|gap| gap.starts_with("a box that names no backticked command")), "{gaps:?}");
    let brief = intake_brief(&new.0, new.1, gaps);
    assert!(brief.starts_with("Item: tompassarelli/north#1632 intake\nCategory: mechanical\n"), "{brief}");
    assert!(brief.contains("Done when: `gh issue view 1632 -R tompassarelli/north"), "{brief}");

    let mut complete = Filer::new(None, false);
    let body = "Category: feature\n\n## Done when\n\n- [ ] the gate passes. Done when: `true` exits 0\n";
    link_with(&created_with("--label enhancement --label priority:now", body, 1633), &mut complete);
    assert!(complete.intakes.is_empty(), "{:?}", complete.intakes);

    let mut worker = Filer::new(None, false);
    worker.may_launch = false;
    let filing = link_with(&created_with("", "Item: x\n\n## Done when\n\n- [ ] the gate passes\n", 1634), &mut worker);
    assert!(worker.intakes.is_empty(), "a worker's filing launches no Haiku pass: {:?}", worker.intakes);
    assert!(matches!(&filing, Verdict::Advise(note) if note.contains("report these gaps to your parent")), "{filing:?}");
}

struct Repo;

impl World for Repo {
    fn repo(&mut self, named: Option<&str>) -> Option<String> {
        named.map(str::to_owned)
    }

    fn claimed(&mut self) -> Vec<String> {
        Vec::new()
    }

    fn refresh_claims(&mut self) {}

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

    fn refresh_claims(&mut self) {}

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
    assert_eq!(decide_with(&creating(&missing), &mut Repo), Verdict::Allow, "a create with no box is filed; the Haiku pass adds one");
}

#[test]
fn a_worker_files_a_child_of_its_own_issue_and_is_refused_an_issue_with_no_parent() {
    let create = |parent: &str| {
        creating(&format!(
            "gh issue create -R tompassarelli/north --label enhancement --title 'Read the leaf row' --body 'Category: feature\n{parent}Done when: `cargo nextest run -p north-guard` exits 0\n'"
        ))
    };
    assert_eq!(decide_with(&create("Parent: north#1137\n"), &mut Worker { filed: 0 }), Verdict::Allow);
    assert_eq!(decide_with(&create(""), &mut Worker { filed: 0 }), Verdict::Allow, "its brief's Done when is recorded, so the issue is not unfounded; the missing box is the Haiku pass's to add");
    assert!(matches!(decide_with(&create("Parent: north#900\n"), &mut Worker { filed: 0 }), Verdict::Deny(reason) if reason.contains("do not hold")));
    let limit = north_base::platform::harness::default_number("worker_children_per_session") as usize;
    assert!(matches!(decide_with(&create("Parent: north#1137\n"), &mut Worker { filed: limit }), Verdict::Deny(reason) if reason.contains("at most")));
    let unrunnable = creating("gh issue create -R tompassarelli/north --label enhancement --title 'Read the leaf row' --body 'Category: feature\nParent: north#1137\nDone when: it works\n'");
    assert_eq!(decide_with(&unrunnable, &mut Worker { filed: 0 }), Verdict::Allow, "a child with an unbacktick Done when is filed; the Haiku pass adds the command");
    let mut lagging = Lagging { refreshed: 0 };
    assert_eq!(decide_with(&create("Parent: north#1137\n"), &mut lagging), Verdict::Allow, "a hold the mirrored snapshot lacks at first is read again after one refresh");
    assert_eq!(lagging.refreshed, 1);
    let mut lagging = Lagging { refreshed: 0 };
    assert!(matches!(decide_with(&create("Parent: north#900\n"), &mut lagging), Verdict::Deny(reason) if reason.contains("do not hold")));
    assert_eq!(lagging.refreshed, 1);
}

struct Lagging {
    refreshed: usize,
}

impl World for Lagging {
    fn repo(&mut self, named: Option<&str>) -> Option<String> {
        named.map(str::to_owned)
    }

    fn claimed(&mut self) -> Vec<String> {
        if self.refreshed == 0 { Vec::new() } else { vec!["north#1137".to_owned()] }
    }

    fn refresh_claims(&mut self) {
        self.refreshed += 1;
    }

    fn open(&mut self, _: &str) -> Vec<(u64, String)> {
        Vec::new()
    }

    fn worker_brief(&mut self) -> Option<String> {
        Some("Item: tompassarelli/north#1137\nDone when: `cargo nextest run -p north-guard` exits 0".to_owned())
    }

    fn children_filed(&mut self) -> usize {
        0
    }
}
