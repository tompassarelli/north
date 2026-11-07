use super::{State, lexical, redirect, strip};

impl State<'_> {
    pub(super) fn check_target(&mut self, text: &str, bounded: bool, cwd: &str) -> Result<(), String> {
        let w = self.expand(strip(text));
        let t = strip(&w);
        if t.contains(['$', '`', '\x02'])
            && !(t == "$HOME"
                || t == "${HOME}"
                || t.starts_with("$HOME/")
                || t.starts_with("${HOME}/"))
        {
            return self.variable_target(t, cwd);
        }
        let Some(p) = self.resolve(&w, cwd) else {
            self.unread = w;
            return Ok(());
        };
        if let Some((hard, why)) = self.classify(&p) {
            return Err(if hard {
                why
            } else if bounded {
                format!("bounded delete (find … -delete, filtered by type/age/name): {why}")
            } else {
                format!("whole-tree recursive delete: {why}")
            });
        }
        Ok(())
    }
    pub(super) fn variable_target(&self, t: &str, cwd: &str) -> Result<(), String> {
        if t.contains("${") && t.contains(":?") {
            return Ok(());
        }
        let disp = t.replace(['"', '\''], "").replace('\x02', "$(…)");
        if t.starts_with('\x02') {
            return Err(format!(
                "command substitution as a recursive-delete target ('{disp}') — its output cannot be checked before it runs. Run the substitution on its own first, then rm -rf the literal path it printed"
            ));
        }
        if t.starts_with('$') {
            return Err(format!(
                "unguarded variable as a recursive-delete target ('{disp}') — an unset variable expands to a bare-root delete. Write the literal path, or guard it: rm -rf \"${{VAR:?}}\"/subdir"
            ));
        }
        let split = t.find(['$', '`', '\x02']).unwrap();
        let pre = &t[..split];
        let pre = if pre.starts_with('/') {
            pre.to_owned()
        } else {
            format!("{}/{pre}", if cwd.is_empty() { &self.cwd } else { cwd })
        };
        let tail = &t[split..];
        if tail.contains("/../") || tail.ends_with("/..") {
            return Err(format!(
                "'..' after a variable in a recursive-delete target ('{disp}') can climb out of '{pre}'. Write the literal path"
            ));
        }
        if !pre.ends_with('/') && self.agent_scratch(&pre) {
            return Ok(());
        }
        let pre = lexical(
            pre.rsplit_once('/')
                .map(|(p, _)| if p.is_empty() { "/" } else { p })
                .unwrap_or("/"),
        );
        if self.scratch(&pre) {
            return Ok(());
        }
        Err(format!(
            "variable expansion inside a recursive-delete target ('{disp}', under '{pre}') — the guard cannot tell what it resolves to. Write the literal path, or guard it: \"${{VAR:?}}\""
        ))
    }
    pub(super) fn rm(&mut self, args: &[String], cwd: &str) -> Result<(), String> {
        self.delete_seen = true;
        let (mut recursive, mut end, mut skip) = (false, false, false);
        let mut targets = Vec::new();
        for t in args {
            if skip {
                skip = false;
                continue;
            }
            if let Some(next) = redirect(t) {
                skip = next;
                continue;
            }
            if end {
                targets.push(t);
            } else if t == "--" {
                end = true;
            } else if t == "--recursive"
                || (t.starts_with('-') && !t.starts_with("--") && t.contains(['r', 'R']))
            {
                recursive = true;
            } else if !t.starts_with('-') {
                targets.push(t);
            }
        }
        if recursive {
            for t in targets {
                match self.glob_files(t, cwd) {
                    Some(files) => files.iter().try_for_each(|file| self.check_target(file, false, cwd))?,
                    None => self.check_target(t, false, cwd)?,
                }
            }
        }
        Ok(())
    }
    fn glob_files(&self, text: &str, cwd: &str) -> Option<Vec<String>> {
        let word = self.expand(strip(text));
        let (directory, pattern) = word.rsplit_once('/')?;
        if !pattern.contains(['*', '?']) || pattern.trim_matches(['*', '?']).is_empty() || directory.contains(['*', '?', '$', '`', '\x02']) {
            return None;
        }
        let directory = self.resolve(directory, cwd)?;
        let names = std::fs::read_dir(&directory).ok()?;
        Some(
            names
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .filter(|name| (pattern.starts_with('.') || !name.starts_with('.')) && wildcard(pattern.as_bytes(), name.as_bytes()))
                .map(|name| format!("{directory}/{name}"))
                .collect(),
        )
    }
    pub(super) fn find(&mut self, args: &[String], cwd: &str) -> Result<(), String> {
        if !args.iter().any(|s| s == "-delete") {
            return Ok(());
        }
        self.delete_seen = true;
        let typef = args
            .windows(2)
            .any(|w| w[0] == "-type" && (w[1] == "f" || w[1].starts_with("f,")));
        let narrow = args.iter().any(|s| {
            [
                "-mtime",
                "-atime",
                "-ctime",
                "-mmin",
                "-amin",
                "-cmin",
                "-size",
                "-name",
                "-iname",
                "-path",
                "-ipath",
                "-regex",
                "-maxdepth",
            ]
            .contains(&s.as_str())
                || s.starts_with("-newer")
        });
        let mut paths = Vec::new();
        for t in args {
            if ["-H", "-L", "-P"].contains(&t.as_str()) || t.starts_with("-O") {
                continue;
            }
            if t.starts_with(['-', '!', '(', '\\']) || t.contains(['<', '>']) {
                break;
            }
            paths.push(t.as_str());
        }
        if paths.is_empty() {
            paths.push(".");
        }
        for t in paths {
            self.check_target(t, typef && narrow, cwd)?;
        }
        Ok(())
    }
}

fn wildcard(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|skip| wildcard(rest, &name[skip..])),
        Some((b'?', rest)) => name.split_first().is_some_and(|(_, tail)| wildcard(rest, tail)),
        Some((byte, rest)) => name.split_first().is_some_and(|(head, tail)| head == byte && wildcard(rest, tail)),
    }
}
