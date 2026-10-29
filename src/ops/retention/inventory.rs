use std::collections::BTreeSet;
use std::io;

use super::family::{safe_name, Family};
use super::remote::Remote;

pub(super) struct Entry {
    pub(super) name: String,
    pub(super) newest: u64,
    pub(super) bytes: u64,
    pub(super) pinned: bool,
    pub(super) opaque: bool,
}

const AWK: &str = r#"{n=split($1,a,"/");k=a[1];if(n==1)kind[k]=$2;else if($2!="f"&&$2!="d"&&$2!="l")odd[k]=1;if($3>m[k])m[k]=$3;b[k]+=$4;if(n==2&&a[2]=="PINNED")p[k]=1}END{for(k in kind)if(kind[k]=="d")printf "%s\t%d\t%d\t%d\t%d\n",k,m[k],b[k],p[k],odd[k]}"#;

pub(super) fn read(remote: &dyn Remote, family: &Family) -> io::Result<Vec<Entry>> {
    let script = format!("{} && find . -mindepth 1 -printf '%P\\t%y\\t%T@\\t%s\\n' | awk -F'\\t' '{AWK}'", family.enter());
    let text = String::from_utf8_lossy(&remote.output(&script)?).into_owned();
    Ok(text.lines().filter_map(parse).filter(|entry| safe_name(&entry.name)).collect())
}

fn parse(line: &str) -> Option<Entry> {
    let mut fields = line.split('\t');
    let name = fields.next()?.to_owned();
    let mut number = || fields.next()?.parse::<u64>().ok();
    let (newest, bytes, pinned, opaque) = (number()?, number()?, number()? != 0, number()? != 0);
    Some(Entry { name, newest, bytes, pinned, opaque })
}

pub(super) fn held(remote: &dyn Remote, family: &Family) -> io::Result<BTreeSet<String>> {
    let script = format!(
        "{} && [ -r /proc/1/cmdline ] && root=$(pwd -P) && echo \"$root\" && {{ find /proc/[0-9]*/cwd /proc/[0-9]*/fd/* -maxdepth 0 -lname \"$root/*\" -printf \"%l\\n\" 2>/dev/null || true; }}",
        family.enter()
    );
    let text = String::from_utf8_lossy(&remote.output(&script)?).into_owned();
    let mut lines = text.lines();
    let root = lines.next().ok_or_else(|| io::Error::other("held read returned nothing"))?;
    let prefix = format!("{root}/");
    Ok(lines.filter_map(|line| line.strip_prefix(&prefix)).filter_map(|rest| rest.split('/').next()).map(str::to_owned).collect())
}
