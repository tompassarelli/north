use super::*;

#[test]
fn recorded_heavy_commands_use_admission_and_preserve_light_commands() {
    let cases: Vec<(String, String)> =
        serde_json::from_str(include_str!("fixtures/heavy_command.json")).unwrap();
    for (command, expected) in cases {
        for source in [
            command.clone(),
            format!("env FLAG=1 nix develop -c bash -lc {}", quote(&command)),
            format!("echo ready && {command}"),
        ] {
            let decision = decide(&source, Path::new("/tmp"), false, 0);
            let observed = match decision {
                Decision::Allow => "allow",
                Decision::Route => "route",
                Decision::Refuse(_) => "refuse",
            };
            assert_eq!(observed, expected, "{source}");
        }
    }
}

#[test]
fn farm_declarations_still_refuse_leased_whole_roster_commands() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(root.path())
            .status()
            .unwrap()
            .success()
    );
    std::fs::create_dir(root.path().join(".agents")).unwrap();
    std::fs::write(root.path().join(".agents/farm-only.toml"), "[[command]]\npattern = ['bun', 'wisp', 'combos', '--advantage']\nnarrowing = ['--fighter']\nfarm = 'farm advantage'\nwhy = 'whole roster'\n").unwrap();
    for prefix in ["", "machine-capacity run -- "] {
        assert!(matches!(
            decide(
                &format!("{prefix}bun wisp combos --advantage"),
                root.path(),
                false,
                0
            ),
            Decision::Refuse(_)
        ));
        assert_eq!(
            decide(
                &format!("{prefix}bun wisp combos --advantage --fighter=Grom"),
                root.path(),
                false,
                0
            ),
            Decision::Allow
        );
    }
    let path = root.path().join(".agents/farm-only.toml");
    let valid = std::fs::read_to_string(&path)
        .unwrap()
        .replace("narrowing = ['--fighter']\n", "");
    std::fs::write(&path, &valid).unwrap();
    assert!(matches!(
        decide("bun wisp combos --advantage", root.path(), false, 0),
        Decision::Refuse(_)
    ));
    std::fs::write(&path, format!("{valid}\n[[command]]\npattern = []\n")).unwrap();
    assert_eq!(
        decide("bun wisp combos --advantage", root.path(), false, 0),
        Decision::Allow
    );
}
