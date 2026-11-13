use super::*;

#[test]
fn only_a_plain_start_request_in_the_request_directory_starts_a_session() {
    let root = tempfile::tempdir().unwrap();
    let (queue, home) = (root.path().join("requests"), root.path().join("agent"));
    let session = queue.join("sessions/w-lead");
    fs::create_dir_all(&session).unwrap();
    let request = |cwd: &str, extra: &str| format!(r#"{{"name":"f-one","account":"a","cwd":"{}/{cwd}","brief":"{}/briefs/one.md"{extra}}}"#, home.display(), home.display());
    fs::write(queue.join("1.start"), request("code/north", "")).unwrap();
    fs::write(queue.join("2.start"), request("code/north", r#","command":"sh -c id""#)).unwrap();
    fs::write(queue.join("3.start"), request("../tom", "")).unwrap();
    std::os::unix::fs::symlink(root.path().join("secret"), queue.join("4.start")).unwrap();
    fs::write(queue.join("5.start"), request("code/north", "").replace(r#""account":"a""#, r#""account":"Api""#)).unwrap();
    fs::write(session.join("6.start"), request("code/north", "").replace("f-one", "f-six")).unwrap();
    let entry = root.path().join("entry");
    fs::write(&entry, "#!/usr/bin/env bash\n! read -t 1 early || exit 6; echo 'north-agent-starts: ready'; read line; read -t 1 more; [ $? -gt 128 ] || exit 7; printf '%s\\n' \"$*\" \"$line\" > \"$0.ran\"\n").unwrap();
    fs::set_permissions(&entry, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let starter = Starter { machinectl: entry, agent: "agent".into(), home: home.clone(), entry: PathBuf::from("/nix/store/entry"), wait: Duration::from_secs(10) };
    let mut log = Vec::new();
    starter.serve_once(&Requests::at(&queue), &mut log).unwrap();
    let log = String::from_utf8(log).unwrap();
    let ran = fs::read_to_string(root.path().join("entry.ran")).unwrap();
    assert_eq!(ran, format!("--quiet --no-ask-password shell agent@.host /nix/store/entry\n{}\n", request("code/north", "")), "{log}");
    assert!(log.contains("refused") && log.contains("2.start") && log.contains("unknown field `command`"), "{log}");
    assert!(log.contains("3.start") && log.contains("is not a plain path beneath"), "{log}");
    assert!(log.contains("set aside") && log.contains("4.start"), "{log}");
    assert!(log.contains("5.start") && log.contains("is an API account"), "{log}");
    assert!(!log.contains("6.start") && session.join("6.start").is_file(), "{log}");
    assert_eq!(Requests::at(&queue).waiting().unwrap(), Vec::<PathBuf>::new());
    assert!(["2.start", "3.start", "4.start", "5.start"].iter().all(|name| fs::symlink_metadata(queue.join(REJECTED).join(name)).is_ok()), "{log}");
    let recorded = record(&mut format!("{}\n", request("code/north", "")).as_bytes(), &mut Vec::new(), &home).unwrap();
    assert_eq!(fs::read_to_string(home.join(RECORDS).join("f-one.json")).unwrap(), recorded.line());
    assert!(record(&mut request("code/north", r#","model":"x""#).as_bytes(), &mut Vec::new(), &home).is_err());
}
