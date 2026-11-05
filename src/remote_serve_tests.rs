use north_core::ops::remote_serve::{Refusal, plan};

#[test]
fn remote_serve_refuses_the_planted_commands() {
    for command in [
        "sh -c id",
        "threads list; id",
        "rsync --server -e.LsfxC . x",
        "north nexus-run -- id",
        "internal-sftp",
        "threads record-priors",
        "env LD_PRELOAD=/x threads list",
        "env 'LAPTOP_BY=$(id)' north laptop status",
        "north laptop open",
    ] {
        assert!(plan(command).is_err(), "{command}");
    }
    assert!(matches!(plan("sh -c id"), Err(Refusal::Verb(_))));
}

#[test]
fn remote_serve_accepts_threads_with_no_native_forced() {
    let planned = plan("env THREADS_ACTOR=w1 threads list").unwrap();
    assert_eq!(planned.argv, ["threads", "list"]);
    assert_eq!(planned.env["THREADS_ACTOR"], "w1");
    assert_eq!(planned.env["THREADS_NO_NATIVE"], "1");
    assert_eq!(plan("threads list").unwrap().env["THREADS_NO_NATIVE"], "1");
}
