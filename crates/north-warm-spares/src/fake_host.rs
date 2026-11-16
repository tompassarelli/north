use std::fs;
use std::path::{Path, PathBuf};

/// Stand-ins for ssh and the remote user manager. The fake ssh relays through two FIFOs to the
/// remote command, which runs in its own session as sshd runs it, so killing the relay drops the
/// transport while the remote side lives on; the fake systemd-run starts a detached process group
/// per unit and systemctl answers for it.
pub fn machine(root: &Path) -> PathBuf {
    let shims = root.join("shims");
    fs::create_dir_all(&shims).unwrap();
    let shim = |name: &str, body: String| {
        let path = shims.join(name);
        north_foundation::foundation::shim::write_script(&path, &format!("SHIMS='{}'\nexport PATH=\"$SHIMS:$PATH\"\n{body}", shims.display()));
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

pub fn alive(pid: &str) -> bool {
    fs::read_to_string(Path::new("/proc").join(pid).join("stat")).is_ok_and(|stat| stat.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z')))
}


pub fn holding(held: &Path) -> Vec<String> {
    let needle = held.to_string_lossy().into_owned();
    let pids = fs::read_dir("/proc").into_iter().flatten().flatten().filter_map(|entry| entry.file_name().to_str().filter(|name| name.chars().all(|c| c.is_ascii_digit())).map(str::to_owned));
    pids.filter(|pid| alive(pid) && fs::read(Path::new("/proc").join(pid).join("cmdline")).is_ok_and(|line| String::from_utf8_lossy(&line).contains(&needle))).collect()
}
