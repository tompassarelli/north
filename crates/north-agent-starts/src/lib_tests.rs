use super::*;

#[test]
fn a_spooled_start_that_carries_a_command_or_reaches_outside_the_agent_home_is_set_aside_unstarted() {
    let root = tempfile::tempdir().unwrap();
    let (spool, home) = (root.path().join("spool"), root.path().join("agent"));
    let location = spool.join("sessions/w-lead");
    fs::create_dir_all(&location).unwrap();
    let request = |cwd: &str, extra: &str| format!(r#"{{"name":"f-one","account":"a","cwd":"{}/{cwd}","brief":"{}/briefs/one.md"{extra}}}"#, home.display(), home.display());
    fs::write(location.join("1.start"), request("code/north", "")).unwrap();
    fs::write(location.join("2.start"), request("code/north", r#","command":"sh -c id""#)).unwrap();
    fs::write(location.join("3.start"), request("../tom", "")).unwrap();
    std::os::unix::fs::symlink(root.path().join("secret"), location.join("4.start")).unwrap();
    let entry = root.path().join("entry");
    fs::write(&entry, "#!/bin/sh\n! read -t 1 early || exit 6; echo 'north-agent-starts: ready'; read line; read -t 1 more; [ $? -gt 128 ] || exit 7; printf '%s\\n' \"$*\" \"$line\" > \"$0.ran\"\n").unwrap();
    fs::set_permissions(&entry, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let starter = Starter { machinectl: entry, agent: "agent".into(), home: home.clone(), entry: PathBuf::from("/nix/store/entry"), wait: Duration::from_secs(10) };
    let mut log = Vec::new();
    starter.serve_once(&Spool::at(&spool), &mut log).unwrap();
    let log = String::from_utf8(log).unwrap();
    let ran = fs::read_to_string(root.path().join("entry.ran")).unwrap();
    assert_eq!(ran, format!("--quiet --no-ask-password shell agent@.host /nix/store/entry\n{}\n", request("code/north", "")), "{log}");
    assert!(log.contains("refused") && log.contains("2.start") && log.contains("unknown field `command`"), "{log}");
    assert!(log.contains("3.start") && log.contains("is not a plain path beneath"), "{log}");
    assert!(log.contains("set aside") && log.contains("4.start"), "{log}");
    assert_eq!(Spool::at(&spool).waiting().unwrap(), Vec::<PathBuf>::new());
    assert!(["w-lead-2.start", "w-lead-3.start", "w-lead-4.start"].iter().all(|name| fs::symlink_metadata(spool.join(REJECTED).join(name)).is_ok()), "{log}");
    let recorded = record(&mut format!("{}\n", request("code/north", "")).as_bytes(), &mut Vec::new(), &home).unwrap();
    assert_eq!(fs::read_to_string(home.join(RECORDS).join("f-one.json")).unwrap(), recorded.line());
    assert!(record(&mut request("code/north", r#","model":"x""#).as_bytes(), &mut Vec::new(), &home).is_err());
}
