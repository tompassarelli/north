use std::collections::BTreeSet;
use std::io::{self, Write};
use std::process::{Command, Stdio};

use super::{ROOT, lowercase_hex};
use crate::verdict_cache::Store;

pub trait Replica {
    fn root(&self) -> String;
    fn run(&self, script: &str, input: &[u8]) -> io::Result<Vec<u8>>;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub sent: usize,
    pub received: usize,
    pub cleared: usize,
}

pub fn exchange(command: &mut Command, input: &[u8]) -> io::Result<Vec<u8>> {
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(|| io::Error::other("no stdin"))?;
    let output = std::thread::scope(|scope| {
        scope.spawn(move || stdin.write_all(input));
        child.wait_with_output()
    })?;
    if output.status.success() { Ok(output.stdout) } else { Err(io::Error::other(format!("{:?} exited {}", command.get_program(), output.status))) }
}

impl Replica for Store {
    fn root(&self) -> String {
        format!("'{}'", self.0.to_string_lossy().replace('\'', "'\\''"))
    }

    fn run(&self, script: &str, input: &[u8]) -> io::Result<Vec<u8>> {
        exchange(Command::new("sh").arg("-c").arg(script), input)
    }
}

fn listed(name: &str) -> bool {
    let parts: Vec<&str> = name.split('/').collect();
    matches!(parts.as_slice(), [weak, "sets" | "pass" | "cleared", strong] if lowercase_hex(weak) && lowercase_hex(strong))
}

fn tombstoned_pass(name: &str) -> Option<String> {
    let (weak, rest) = name.split_once('/')?;
    rest.strip_prefix("cleared/").map(|strong| format!("{weak}/pass/{strong}"))
}

fn names(replica: &dyn Replica) -> io::Result<BTreeSet<String>> {
    let script = format!("cd {}/{ROOT} 2>/dev/null || exit 0; find . -type f ! -name '.*' | sed 's|^\\./||'", replica.root());
    Ok(String::from_utf8_lossy(&replica.run(&script, b"")?).lines().filter(|name| listed(name)).map(str::to_owned).collect())
}

fn lines(names: &[&String]) -> Vec<u8> {
    names.iter().flat_map(|name| format!("{name}\n").into_bytes()).collect()
}

fn copy(from: &dyn Replica, to: &dyn Replica, wanted: &[&String]) -> io::Result<()> {
    if wanted.is_empty() {
        return Ok(());
    }
    let archive = from.run(&format!("cd {}/{ROOT} && tar -cf - -T -", from.root()), &lines(wanted))?;
    let root = to.root();
    let script = format!(
        "d={root}; mkdir -p \"$d\"; t=$(mktemp -d \"$d/.incoming.XXXXXX\") || exit 1; tar -C \"$t\" -xf - || exit 1; cd \"$t\" || exit 1; \
         find . -type f | while IFS= read -r f; do f=${{f#./}}; mkdir -p \"$d/{ROOT}/${{f%/*}}\"; mv -n \"$f\" \"$d/{ROOT}/$f\"; done; cd / && rm -r -- \"$t\""
    );
    to.run(&script, &archive).map(|_| ())
}

fn remove(replica: &dyn Replica, dead: &[&String]) -> io::Result<()> {
    if dead.is_empty() {
        return Ok(());
    }
    replica.run(&format!("cd {}/{ROOT} && while IFS= read -r n; do rm -f -- \"$n\"; done", replica.root()), &lines(dead)).map(|_| ())
}

pub fn sync(here: &dyn Replica, there: &dyn Replica) -> io::Result<Report> {
    let (ours, theirs) = (names(here)?, names(there)?);
    let tombstoned: BTreeSet<String> = ours.union(&theirs).filter_map(|name| tombstoned_pass(name)).collect();
    let dead = |set: &BTreeSet<String>| -> Vec<String> { tombstoned.iter().filter(|name| set.contains(*name)).cloned().collect() };
    let (dead_here, dead_there) = (dead(&ours), dead(&theirs));
    let carried = |from: &BTreeSet<String>, to: &BTreeSet<String>| -> Vec<String> { from.difference(to).filter(|name| !tombstoned.contains(*name)).cloned().collect() };
    let (to_there, to_here) = (carried(&ours, &theirs), carried(&theirs, &ours));
    copy(here, there, &to_there.iter().collect::<Vec<_>>())?;
    copy(there, here, &to_here.iter().collect::<Vec<_>>())?;
    remove(here, &dead_here.iter().collect::<Vec<_>>())?;
    remove(there, &dead_there.iter().collect::<Vec<_>>())?;
    Ok(Report { sent: to_there.len(), received: to_here.len(), cleared: dead_here.len() + dead_there.len() })
}
