use std::os::unix::net::UnixStream;

pub fn vet(stream: UnixStream) -> Result<UnixStream, String> {
    let peer = rustix::net::sockopt::get_socket_peercred(&stream).map_err(|error| format!("cannot read who answers the user broker socket: {error}"))?;
    if peer.uid != rustix::process::getuid() {
        return Err(format!("the user broker socket is answered by uid {}, not this user", peer.uid.as_raw()));
    }
    let own = no_new_privs("self").unwrap_or(false);
    let theirs = no_new_privs(&peer.pid.as_raw_nonzero().get().to_string()).ok_or_else(|| "cannot read the user broker's process".to_owned())?;
    refuse_confined_peer(own, theirs).map(|()| stream)
}

fn refuse_confined_peer(own_confined: bool, peer_confined: bool) -> Result<(), String> {
    if peer_confined && !own_confined {
        return Err("the user broker socket is answered by a confined process (a worker sandbox), not by a broker this user started".to_owned());
    }
    Ok(())
}

fn no_new_privs(process: &str) -> Option<bool> {
    let status = std::fs::read_to_string(format!("/proc/{process}/status")).ok()?;
    status.lines().find_map(|line| line.strip_prefix("NoNewPrivs:")).map(|value| value.trim() == "1")
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    #[test]
    fn an_unconfined_client_refuses_a_user_socket_a_confined_child_bound_first() {
        let mut child = Command::new("setpriv").args(["--no-new-privs", "sleep", "30"]).stdin(Stdio::null()).spawn().unwrap();
        let id = child.id().to_string();
        let confined = crate::platform::wait::until(std::time::Duration::from_secs(5), || super::no_new_privs(&id) == Some(true));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(confined);
        assert!(super::refuse_confined_peer(false, true).is_err());
        assert!(super::refuse_confined_peer(false, false).is_ok());
        assert!(super::refuse_confined_peer(true, true).is_ok());
    }
}
