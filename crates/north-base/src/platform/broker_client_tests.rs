use super::connect_shared;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixListener;

#[test]
fn a_caller_locked_out_of_the_system_socket_reaches_the_agent_socket() {
    let dir = tempfile::tempdir().unwrap();
    let agent = dir.path().join("agent.sock");
    let _listener = UnixListener::bind(&agent).unwrap();
    let locked = dir.path().join("north-broker");
    std::fs::DirBuilder::new().mode(0o600).create(&locked).unwrap();
    assert!(connect_shared(&locked.join("broker.sock"), &agent).unwrap().is_some());
    let refusing = dir.path().join("system.sock");
    std::fs::write(&refusing, b"").unwrap();
    assert!(connect_shared(&refusing, &agent).unwrap_err().contains("exists and does not answer this user"));
}
