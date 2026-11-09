use std::fs;

use north_base::foundation::shim::write_executable;

use super::serve::{World, git, report};

const BRANCH: &str = "q902-deadcode";

fn gates(answered_by: &str) -> String {
    serde_json::json!([{
        "id": 41,
        "question": format!("Land {BRANCH} by break-glass, straight to main without a lander round?"),
        "options": ["Yes: break glass for it", "No: let it wait for the lander"],
        "answer": "Yes: break glass for it",
        "answered_by": answered_by,
    }])
    .to_string()
}

#[test]
fn break_glass_lands_a_ticket_straight_onto_main_after_the_secret_scan_and_records_an_incident_only_under_an_approval_tom_answered_himself() {
    let world = World::new();
    let (bin, calls) = (world.path().join("bin"), world.path().join("bin/north.calls"));
    let gate_file = world.path().join("gates.json");
    write_executable(&bin.join("north"), format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\n[ \"$1 $2\" = 'gate list' ] && cat '{}'\nexit 0\n", gate_file.display()));
    write_executable(&bin.join("betterleaks"), "#!/bin/sh\nprintf 'betterleaks %s\\n' \"$*\" >> \"$(dirname \"$0\")/north.calls\"\n");
    let repository = world.repository("north");
    let start = git(&repository.checkout, &["rev-parse", "HEAD"]);
    repository.lane(BRANCH);
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let break_glass = || {
        let mut command = world.land(&["break-glass", BRANCH, "--reason", "the lander cannot land its own fix", "--approval", "41", "--repo", repository.checkout.to_str().unwrap(), "--app-token", world.path().join("bin/token").to_str().unwrap()]);
        command.env("PATH", &path).output().unwrap()
    };

    fs::write(&gate_file, gates("default")).unwrap();
    let refused = break_glass();
    let transcript = report(&refused);
    assert_eq!(refused.status.code(), Some(1), "a gate the default answered is no approval of Tom's\n{transcript}");
    assert!(transcript.contains("no recorded approval from Tom"), "{transcript}");
    assert_eq!(git(&repository.origin, &["rev-parse", "main"]), start, "nothing is pushed without approval\n{transcript}");
    assert!(!fs::read_to_string(&calls).unwrap_or_default().contains("betterleaks"), "{transcript}");

    fs::write(&gate_file, gates("tom")).unwrap();
    let landed = break_glass();
    let transcript = report(&landed);
    assert_eq!(landed.status.code(), Some(0), "{transcript}");
    let main = git(&repository.origin, &["log", "-1", "--format=%P %s", "main"]);
    assert!(main.starts_with(&format!("{start} {BRANCH} ")), "the ticket is pushed straight onto main: {main}\n{transcript}");
    let logged = fs::read_to_string(&calls).unwrap_or_default();
    assert!(logged.lines().any(|line| line.starts_with("betterleaks ") && line.contains(&start)), "the secret scan reads the exact commits pushed\n{logged}\n{transcript}");
    let incident = logged.lines().find(|line| line.starts_with("ledger incident record") && line.contains("--class lander-break-glass")).unwrap_or_default();
    assert!(incident.contains("--noticed-by tom") && incident.contains("the lander cannot land its own fix"), "the incident names the reason\n{logged}\n{transcript}");
    assert!(!fs::read_dir(&repository.queue).unwrap().flatten().any(|entry| entry.path().extension().is_some_and(|extension| extension == "json")), "the ticket leaves the queue as landed\n{transcript}");
}
