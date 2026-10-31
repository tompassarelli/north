use super::transfer::secret;
use super::{ASSIGNMENT, State, base, sep, strip};

#[derive(Default)]
struct Mktemp {
    dir: String,
    template: String,
    tflag: bool,
    want: bool,
}

fn mktemp_option(t: &str, m: &mut Mktemp) {
    match t {
        "-p" | "--tmpdir" => m.want = true,
        "-t" => m.tflag = true,
        _ if t.starts_with("-p") && t.len() > 2 => m.dir = t[2..].into(),
        _ if t.starts_with("--tmpdir=") => m.dir = t[9..].into(),
        _ if t.starts_with('-') => {}
        _ => m.template = t.into(),
    }
}

fn unquote_value(value: &str) -> Option<&str> {
    let quoted = |q: char| value.starts_with(q) && value.ends_with(q) && value.len() >= 2;
    if quoted('\'') {
        let inner = &value[1..value.len() - 1];
        return (!inner.contains(['$', '\x02'])).then_some(inner);
    }
    if quoted('"') {
        return Some(&value[1..value.len() - 1]);
    }
    if value.starts_with(['\'', '"']) || value.ends_with(['\'', '"']) {
        return None;
    }
    Some(value)
}

fn power(word: &str, args: &[String]) -> Result<(), String> {
    match word {
        "mkfs" => Err("mkfs — formatting filesystems is manual".into()),
        _ if word.starts_with("mkfs.") => Err("mkfs — formatting filesystems is manual".into()),
        "shutdown" | "reboot" | "poweroff" | "halt" => {
            Err(format!("{word} — system power ops are manual"))
        }
        "systemctl" if !args.iter().any(|s| s == "--user") => systemctl(args),
        _ => Ok(()),
    }
}

fn systemctl(args: &[String]) -> Result<(), String> {
    if let Some(sub) = args.iter().find(|s| !s.starts_with('-'))
        && [
            "poweroff",
            "reboot",
            "halt",
            "kexec",
            "suspend",
            "hibernate",
        ]
        .contains(&sub.as_str())
        {
            return Err(format!("systemctl {sub} — system power ops are manual"));
        }
    Ok(())
}

fn raw_device(args: &[String]) -> Result<(), String> {
    for t in args {
        let s = strip(t);
        if s.starts_with("of=/dev/")
            && !["of=/dev/null", "of=/dev/stdout", "of=/dev/stderr"].contains(&s)
        {
            return Err(format!(
                "dd of={} — writing raw devices is destructive",
                &s[3..]
            ));
        }
    }
    Ok(())
}

fn recursive_permissions(word: &str, args: &[String]) -> Result<(), String> {
    if !args
        .iter()
        .any(|s| s == "--recursive" || (s.starts_with('-') && s.contains('R')))
    {
        return Ok(());
    }
    let Some(operand) = args.iter().find(|s| !s.starts_with('-')) else {
        return Ok(());
    };
    if word == "chmod" && ["000", "0000"].contains(&operand.as_str()) {
        return Err(format!("chmod -R {operand} — recursive permission wipe"));
    }
    if word == "chown" && (operand == "root" || operand.starts_with("root:")) {
        return Err(format!(
            "chown -R {operand} — recursive root takeover of a tree"
        ));
    }
    Ok(())
}

impl State<'_> {
    fn mktemp(&self, mut k: usize) -> Option<Mktemp> {
        if self.tokens.get(k)?.as_str() != ";" || self.tokens.get(k + 1)?.as_str() != "(" {
            return None;
        }
        k += 2;
        let mut last = self.tokens.get(k)?.contains(')');
        if base(strip(self.tokens.get(k)?)) != "mktemp" {
            return None;
        }
        let mut m = Mktemp::default();
        while !last {
            k += 1;
            let token = self.tokens.get(k)?;
            last = token.contains(')');
            if sep(token) {
                return None;
            }
            let t = strip(token);
            if m.want {
                m.dir = t.into();
                m.want = false;
                continue;
            }
            mktemp_option(t, &mut m);
        }
        Some(m)
    }
    fn fresh_temp(&self, k: usize) -> Option<String> {
        let m = self.mktemp(k)?;
        let tmp = if self.tmp.is_empty() {
            "/tmp"
        } else {
            &self.tmp
        };
        let dir = self.tilde(if m.want { tmp } else { &m.dir });
        let name = if m.template.is_empty() {
            "tmp.XXXXXXXXXX"
        } else {
            &m.template
        };
        let fresh = if !dir.is_empty() {
            format!("{dir}/{name}")
        } else if !m.template.is_empty() && !m.tflag {
            m.template.clone()
        } else {
            format!("{tmp}/{name}")
        };
        Some(self.expand(&fresh))
    }
    fn assignment_start(&self, start: usize, end: usize) -> Option<usize> {
        let mut k = start;
        let at = |k: usize, words: &[&str]| {
            self.tokens
                .get(k)
                .is_some_and(|t| words.contains(&t.as_str()))
        };
        if at(k, &["do", "then", "else", "{", "!"]) {
            k += 1;
        }
        if at(k, &["export", "local", "declare", "readonly", "typeset"]) {
            k += 1;
        }
        let plain = k < end
            && self.tokens[k..end]
                .iter()
                .all(|t| ASSIGNMENT.is_match(t) || t.starts_with('-'));
        plain.then_some(k)
    }
    fn assign(&mut self, token: &str, end: usize, next: &str) {
        let (name, value) = token.split_once('=').unwrap();
        if ["||", "|"].contains(&self.previous.as_str()) || next == "|" {
            self.vars.remove(name);
            return;
        }
        let value = if value == "\x02" || value == "\"\x02" {
            self.fresh_temp(end).unwrap_or_else(|| "\x02".into())
        } else {
            let Some(value) = unquote_value(value) else {
                self.vars.remove(name);
                return;
            };
            self.expand(&self.tilde(value))
        };
        self.vars.insert(name.into(), (value, self.scopes.len()));
    }
    pub(super) fn assignments(&mut self, start: usize, end: usize, next: &str) -> bool {
        let Some(k) = self.assignment_start(start, end) else {
            return false;
        };
        for j in k..end {
            let token = self.tokens[j].clone();
            if !token.starts_with('-') {
                self.assign(&token, end, next);
            }
        }
        true
    }
    fn cd_destination<'s>(&self, args: &'s [String]) -> Option<&'s str> {
        let mut dest = "~";
        for t in args {
            if t == "-" {
                return None;
            }
            if t.starts_with('-') {
                continue;
            }
            dest = t;
            break;
        }
        (!dest.contains(['*', '?']) && !dest.starts_with('+')).then_some(dest)
    }
    pub(super) fn cd(&mut self, args: &[String], next: &str) {
        if self.previous == "|" || next == "|" {
            return;
        }
        let Some(dest) = self.cd_destination(args) else {
            return;
        };
        let mut moved = Vec::new();
        for cwd in &self.cwds {
            let Some(p) = self.resolve(dest, cwd) else {
                return;
            };
            self.cd_dirs.push(p.clone());
            moved.push(p);
        }
        if next == "&&" {
            self.cwds = moved;
            return;
        }
        for p in moved {
            if !self.cwds.contains(&p) {
                self.cwds.push(p);
            }
        }
    }
    fn deletes(&mut self, word: &str, args: &[String]) -> Result<(), String> {
        for cwd in self.cwds.clone() {
            match word {
                "rm" => self.rm(args, &cwd)?,
                "find" => self.find(args, &cwd)?,
                _ => self.git(args, &cwd)?,
            }
        }
        Ok(())
    }
    fn ssh(&self, index: usize) -> Result<(), String> {
        for t in self.tokens[..index].iter().rev() {
            if [";", "&&", "||"].contains(&t.as_str()) {
                break;
            }
            if t != "|" && secret(strip(t)) {
                return Err("secret path piped into ssh — local credential material into ssh stdin is an exfil surface (remote reads inside ssh's own args stay allowed)".into());
            }
        }
        Ok(())
    }
    pub(super) fn dispatch(
        &mut self,
        word: &str,
        args: &[String],
        index: usize,
    ) -> Result<(), String> {
        match word {
            "cd" | "pushd" => Ok(()),
            "rm" | "find" | "git" => self.deletes(word, args),
            "rmdir" | "unlink" => {
                self.delete_seen = true;
                Ok(())
            }
            "read" | "unset" | "mapfile" | "readarray" => {
                for a in args {
                    self.vars.remove(a);
                }
                Ok(())
            }
            "curl" | "wget" => self.http(word, args),
            "gh" => self.gh(args),
            "nc" | "ncat" | "netcat" => self.exfil(word),
            "ssh" if !self.localhost => self.ssh(index),
            "scp" | "rsync" => self.copy(word, args),
            "dd" => raw_device(args),
            "chmod" | "chown" => recursive_permissions(word, args),
            _ => power(word, args),
        }
    }
}
