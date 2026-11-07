use super::*;

struct Filer {
    brief: String,
    claimed: Vec<Ref>,
}

impl Filed for Filer {
    fn connect(&mut self, _: &Ref, _: &Ref) -> Option<()> {
        Some(())
    }

    fn tidy(&mut self, _: &Ref) -> Result<bool, String> {
        Ok(false)
    }

    fn brief(&mut self) -> Option<String> {
        Some(self.brief.clone())
    }

    fn claim(&mut self, new: &Ref) -> Result<String, String> {
        self.claimed.push(new.clone());
        Ok(format!("owns {}#{}", new.0, new.1))
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
fn an_issue_a_worker_files_for_its_own_brief_is_claimed_for_it_and_a_blocker_is_not() {
    let replay = [
        (737, "api_pool::open draws the grant with credit left that expires first"),
        (739, "the firn follow timer ships each landing to every host it serves"),
        (736, "remote-serve refuses an ssh command outside its allowlist"),
    ];
    for (number, done_when) in replay {
        let brief = format!("Item: new north issue, file it first\nDone when: {done_when}\n");
        let mut filer = Filer { brief: brief.clone(), claimed: Vec::new() };
        let verdict = link_with(&filed(number, &format!("Category: tooling\n\n## Done when\n- [ ] {done_when}")), &mut filer);
        assert_eq!(filer.claimed, [("tompassarelli/north".to_owned(), number)], "north#{number} was staffed beside the session that filed it");
        assert!(matches!(verdict, Verdict::Advise(note) if note.contains("not staff it again")), "north#{number}");
        let mut blocker = Filer { brief, claimed: Vec::new() };
        link_with(&filed(number + 100, &format!("Blocks: north#{number}\nCategory: tooling\n\n## Done when\n- [ ] {done_when}")), &mut blocker);
        assert!(blocker.claimed.is_empty(), "a blocker north#{number} files stays for dispatch");
    }
}
