use super::RUN_TIME_READERS;

#[test]
fn the_defaults_reader_refuses_a_route_the_router_would_panic_on() {
    run_time_data_install();
    let entry = "[codex_route_newcat]\nvalue = 'gpt'\nallowed = { prefix = 'g' }\n";
    let findings = north_core::run_time_data::check("harness/defaults.toml", None, Some(entry));
    assert!(findings.iter().any(|finding| finding.reason.contains("codex_route_newcat: unknown value gpt")), "{findings:?}");
}

fn run_time_data_install() {
    north_core::run_time_data::install(&RUN_TIME_READERS);
}

#[test]
fn no_brief_plan_or_gate_needs_a_human_sign_in_under_the_shipped_phrase_lists() {
    use north_core::fleet::gates::GateRequest;
    use north_core::platform::signin::brief_refusal as signin_refusal;

    let brief = |box_line: &str| format!("Item: north#9 Mirror access\nBox: {box_line}\nCategory: feature\nDone when: `north credentials check` exits 0\nFiles: crates/north-ops/src/ops/wc3_login_field/mod.rs\n\nGoal: a sign-in named here, after the header, is not scanned.");
    for needs in ["Sign in to GitHub in the browser and create a personal access token", "Confirm the passkey prompt, then run the check", "Unlock the keyring and type the password", "Log in as Tom on the Hive Workshop"] {
        let why = signin_refusal(&brief(needs)).unwrap_or_else(|| panic!("{needs} launched"));
        assert!(why.contains("machine path"), "{why}");
    }
    for builds in ["No agent work depends on a human sign-in, by construction", "Replace the personal token with an App token so nobody signs in again; never ask for a passkey", "Mint installation tokens from the sops key without a browser login", "Rename the module"] {
        assert_eq!(signin_refusal(&brief(builds)), None, "{builds}");
    }
    let ask = |question: &str, options: [&str; 2]| GateRequest { verification: false, issue: None, box_text: None, session: "worker".into(), question: question.into(), options: options.map(String::from).to_vec(), recommend: 1, default: 2, default_after_ms: 30_000 };
    for refused in [ask("Tom, please sign in to GitHub so the box can continue?", ["done", "later"]), ask("Which passkey confirmation do you want?", ["this one", "that one"]), ask("Unlock the keyring?", ["yes", "no"]), ask("Which route?", ["log in through the browser", "wait"])] {
        let why = refused.validate().unwrap_err().user_message();
        assert!(why.contains("machine path"), "{}: {why}", refused.question);
    }
    let new_account = GateRequest { default: 0, default_after_ms: 0, ..ask("Create a new account for the mirror and sign in to it?", ["create it", "skip"]) };
    assert!(new_account.validate().is_ok());
    assert!(ask("Rename the module?", ["yes", "no"]).validate().is_ok());
}
