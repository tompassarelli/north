use north_ops::ops::remote_serve::{Refusal, plan};

#[test]
fn remote_serve_refuses_the_planted_commands() {
    for command in [
        "sh -c id",
        "threads list; id",
        "rsync --server -e.LsfxC . x",
        "north capacity nexus -- id",
        "internal-sftp",
        "threads record-priors",
        "env LD_PRELOAD=/x threads list",
        "env 'LAPTOP_BY=$(id)' north fleet laptop status",
        "north fleet laptop open",
    ] {
        assert!(plan(command).is_err(), "{command}");
    }
    assert!(matches!(plan("sh -c id"), Err(Refusal::Verb(_))));
}
