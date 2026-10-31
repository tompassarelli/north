use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};

use super::super::model::Machine;
use super::super::remote::held_under;
use super::super::runtime::HostSpec;
use super::*;

const WAIT: Duration = Duration::from_secs(20);

/// Stand-ins for ssh and the remote user manager. The fake ssh relays through two FIFOs to the
/// remote command, which runs in its own session as sshd runs it, so killing the relay drops the
/// transport while the remote side lives on; the fake systemd-run starts a detached process group
/// per unit and systemctl answers for it.
fn machine(root: &Path) -> PathBuf {
    let shims = root.join("shims");
    fs::create_dir_all(&shims).unwrap();
    let shim = |name: &str, body: String| {
        let path = shims.join(name);
        fs::write(&path, format!("#!/bin/sh\nSHIMS='{}'\nexport PATH=\"$SHIMS:$PATH\"\n{body}", shims.display())).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    };
    shim(
        "ssh",
        r#"while [ "$1" != -- ]; do shift; done
t=$(mktemp -d "$SHIMS/link.XXXXXX"); mkfifo "$t/i" "$t/o"
exec 3<&0
cat <&3 > "$t/i" & up=$!
cat "$t/o" & down=$!
echo "$$ $up $down" >> "$SHIMS/links"
setsid sh -c "$2" < "$t/i" > "$t/o"
code=$?
kill "$up" 2>/dev/null; wait "$down"
exit "$code"
"#
        .into(),
    );
    shim(
        "systemd-run",
        r#"for a; do case $a in --unit=*) unit=${a#--unit=};; --) break;; esac; done
while [ "$1" != -- ]; do case $1 in --setenv=*) export "${1#--setenv=}";; esac; shift; done
shift
setsid "$@" < /dev/null > /dev/null 2>&1 &
echo $! > "$SHIMS/unit-$unit"
"#
        .into(),
    );
    shim(
        "systemctl",
        r#"for a; do unit=$a; done
pid=$(cat "$SHIMS/unit-$unit" 2>/dev/null)
case "$*" in
  *stop*) [ -n "$pid" ] && kill -s TERM -- "-$pid" 2>/dev/null; exit 0;;
  *show*) if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then echo "$pid"; else echo 0; fi;;
  *is-active*) [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null;;
esac
"#
        .into(),
    );
    shims
}

fn held(root: &Path, argv: &[&str], env: Vec<(String, std::ffi::OsString)>) -> Link {
    let shims = machine(root);
    let spec = HostSpec { key: "box one".into(), shared: false, argv: argv.iter().map(|word| (*word).to_owned()).collect(), env, env_remove: Vec::new(), cwd: root.to_path_buf() };
    let placed = held_under(spec, &Machine { name: "far".into(), ssh: "tom@far".into() }, root).unwrap();
    assert_eq!(placed.argv[1], "session-link");
    Link { ssh: shims.join("ssh"), ..Link::parse(&placed.argv[2..]).unwrap() }
}

async fn next(lines: &mut Lines<BufReader<DuplexStream>>) -> String {
    tokio::time::timeout(WAIT, lines.next_line()).await.unwrap().unwrap().unwrap()
}

fn alive(pid: &str) -> bool {
    fs::read_to_string(Path::new("/proc").join(pid).join("stat")).is_ok_and(|stat| stat.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z')))
}

fn drop_transport(shims: &Path) {
    let links = fs::read_to_string(shims.join("links")).unwrap();
    for pid in links.lines().last().unwrap().split_whitespace() {
        let _ = std::process::Command::new("kill").args(["-9", pid]).status();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_session_survives_its_transport_dying_mid_wait() {
    let root = tempfile::tempdir().unwrap();
    let cli = r#"while read -r line; do case $line in wait) (sleep 2; echo "waited $$") & ;; *) echo "got $line $$";; esac; done"#;
    let link = held(root.path(), &["sh", "-c", cli], Vec::new());
    let (mut to_link, input) = tokio::io::duplex(4096);
    let (mut output, from_link) = tokio::io::duplex(4096);
    let relay = tokio::spawn(async move { link.relay(input, &mut output, std::future::pending()).await });
    let mut lines = BufReader::new(from_link).lines();

    to_link.write_all(b"one\n").await.unwrap();
    let first = next(&mut lines).await;
    let cli_pid = first.strip_prefix("got one ").unwrap().to_owned();
    to_link.write_all(b"wait\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop_transport(&root.path().join("shims"));
    to_link.write_all(b"two\n").await.unwrap();

    let mut after = vec![next(&mut lines).await, next(&mut lines).await];
    after.sort();
    assert_eq!(after, [format!("got two {cli_pid}"), format!("waited {cli_pid}")], "the background wait and the input sent while the link was down both reach the same CLI, once each");
    let links = fs::read_to_string(root.path().join("shims/links")).unwrap();
    assert!(links.lines().count() >= 2, "the link reconnected: {links}");
    assert!(alive(&cli_pid));

    drop(to_link);
    assert_eq!(tokio::time::timeout(WAIT, relay).await.unwrap().unwrap().unwrap(), 0);
    let deadline = std::time::Instant::now() + WAIT;
    while alive(&cli_pid) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!alive(&cli_pid), "closing the host's input stops the held service");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_held_cli_receives_every_argument_and_variable_verbatim_and_its_exit_ends_the_link() {
    let root = tempfile::tempdir().unwrap();
    let tricky = "it's \"quoted\" $HOME `id` \\ ; && newline\nend";
    let probe = r#"printf '%s|%s|%s' "$PWD" "$NORTH_REMOTE_PROBE" "$1"; exit 3"#;
    let link = held(root.path(), &["sh", "-c", probe, "sh", tricky], vec![("NORTH_REMOTE_PROBE".into(), tricky.into())]);
    let (_to_link, input) = tokio::io::duplex(64);
    let (mut output, mut from_link) = tokio::io::duplex(4096);
    let code = tokio::time::timeout(WAIT, link.relay(input, &mut output, std::future::pending())).await.unwrap().unwrap();
    drop(output);
    let mut printed = String::new();
    from_link.read_to_string(&mut printed).await.unwrap();
    assert_eq!(printed, format!("{}|{tricky}|{tricky}", root.path().display()));
    assert_eq!(code, 3);
}
