use std::os::unix::fs::PermissionsExt;

use super::*;

#[test]
fn a_command_through_a_spare_answers_with_its_output_and_exit_status_as_ssh_would() {
    let dir = tempfile::tempdir().expect("tempdir");
    let held = dir.path().join("spares");
    let serving = held.clone();
    std::thread::spawn(move || holding(&serving, "build-host", || Command::new("sh").args(["-c", WAITING]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn(), || None));
    let mut taken = None;
    let handed = north_base::platform::wait::until(Duration::from_secs(10), || {
        taken = UnixStream::connect(socket(&held, "build-host")).ok().as_ref().and_then(received);
        taken.is_some()
    });
    assert!(handed, "no spare was handed out within 10 s");
    assert_eq!(finished(taken.expect("handed"), "printf 'one\\ntwo'; false", Duration::from_secs(5)), Some((1, "one\ntwo".to_owned())));

    let mode = |path: &Path| fs::metadata(path).expect("mode").permissions().mode() & 0o777;
    assert_eq!((mode(&held), mode(&socket(&held, "build-host"))), (0o700, 0o600), "the socket that hands out a shell on the build host is its owner's alone");
    let wide = dir.path().join("wide");
    fs::create_dir(&wide).expect("wide");
    fs::set_permissions(&wide, fs::Permissions::from_mode(0o755)).expect("chmod");
    let refused = holding(&wide, "build-host", || Err(io::Error::other("never opened")), || None).expect_err("a directory others can read");
    assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied, "a holder refuses to serve from a directory wider than its owner: {refused}");

    let serving = held.clone();
    std::thread::spawn(move || holding(&serving, "unreachable-host", || Err(io::Error::other("the host does not answer")), || None));
    let empty = socket(&held, "unreachable-host");
    assert!(north_base::platform::wait::until(Duration::from_secs(10), || empty.exists()), "the empty holder never listened");
    assert!(UnixStream::connect(&empty).ok().as_ref().and_then(received).is_none(), "a holder with no spare ready hands out nothing, so the caller runs plain ssh");
}
