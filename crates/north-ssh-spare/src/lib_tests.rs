use super::*;

#[test]
fn a_command_through_a_spare_answers_with_its_output_and_exit_status_as_ssh_would() {
    let dir = tempfile::tempdir().expect("tempdir");
    let held = dir.path().to_path_buf();
    std::thread::spawn(move || holding(&held, "build-host", || Command::new("sh").args(["-c", WAITING]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn(), || None));
    let mut taken = None;
    let handed = north_base::platform::wait::until(Duration::from_secs(10), || {
        taken = UnixStream::connect(socket(dir.path(), "build-host")).ok().as_ref().and_then(received);
        taken.is_some()
    });
    assert!(handed, "no spare was handed out within 10 s");
    assert_eq!(finished(taken.expect("handed"), "printf 'one\\ntwo'; false", Duration::from_secs(5)), Some((1, "one\ntwo".to_owned())));
}
