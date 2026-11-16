use std::collections::BTreeMap;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use landlock::RulesetStatus;
use north_base::platform::broker_client::Client;
use north_broker::{Broker, Policy, Source, Surface};

use super::{Context, Reader, confine, executable, readable, removable};

const SOCKET: &str = "NORTH_1397_KEPT_BROKER";
const NAME: &str = "worker_sandbox::tests::kept_socket::a_kept_socket_answers_a_confined_command_with_no_more_than_the_workers_reach";
const REFUSED: &str = "the lander App goes only to a caller without NoNewPrivs";

fn asked(client: &Client, app: &str) -> String {
    client.github_token(app, &[], &BTreeMap::new(), 0).map_or_else(|why| why, |grant| format!("granted until {}", grant.expires_at))
}

#[test]
fn a_kept_socket_answers_a_confined_command_with_no_more_than_the_workers_reach() {
    if let Some(socket) = std::env::var_os(SOCKET) {
        let context = Context::from_env("north broker github-token");
        let roots = removable(&context);
        assert!(matches!(confine(Some(&roots), &readable(&context, Reader::of(true)), Some(&executable(&context)), None, &[]), Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
        let client = Client::at(Path::new(&socket));
        println!("lander\t{}\nagent\t{}", asked(&client, "lander"), asked(&client, "agent"));
        return;
    }
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-1397-kept").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let keys = home.join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    let socket = home.join("broker.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let broker = Arc::new(Broker::new(Policy::for_user(rustix::process::getuid().as_raw()), Source::Directory(keys)));
    std::thread::spawn(move || broker.serve(&listener, Surface::Open));
    let output = Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"]).env_clear().env("PATH", std::env::var_os("PATH").unwrap()).env("HOME", &home).env(SOCKET, &socket).current_dir(&home).output().unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success() && stdout.contains("running 1 test"), "the confined child did not run: {stdout}{stderr}");
    let answer = |app: &str| stdout.lines().find_map(|line| line.strip_prefix(&format!("{app}\t"))).unwrap_or_default().to_owned();
    assert!(answer("lander").contains(REFUSED), "the credential broker answered a confined command's lander token ask with more than a refusal: {stdout}");
    assert!(!answer("agent").is_empty() && !answer("agent").contains("NoNewPrivs"), "a confined command's own agent token ask was refused for its confinement: {stdout}");
}
