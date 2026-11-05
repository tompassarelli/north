use super::{GIT_DATA, MUTATION, OVERRIDE, State, git, lexical, strip, under};
use regex::Regex;

#[derive(Default)]
pub(super) struct Request<'s> {
    pub(super) method: &'s str,
    pub(super) target: &'s str,
    pub(super) body: bool,
    pub(super) mutation: bool,
    pub(super) upload: bool,
}

impl Request<'_> {
    pub(super) fn method(&self) -> &str {
        match (self.method.is_empty(), self.body) {
            (false, _) => self.method,
            (true, true) => "POST",
            (true, false) => "GET",
        }
    }
}

fn global_options(args: &[String]) -> (&str, usize) {
    let mut i = 0;
    let mut directory = "";
    while i < args.len() {
        let t = args[i].as_str();
        match t {
            "-C" => {
                i += 1;
                directory = args.get(i).map(String::as_str).unwrap_or("");
            }
            "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--exec-path" => i += 1,
            _ if t.starts_with('-') => {}
            _ => break,
        }
        i += 1;
    }
    (directory, i)
}

fn forced(argument: &str) -> bool {
    [
        "--force",
        "--force-with-lease",
        "--force-if-includes",
        "--mirror",
        "--prune",
    ]
    .contains(&argument)
        || argument.starts_with("--force-with-lease=")
        || argument.starts_with('+')
        || (argument.starts_with('-') && !argument.starts_with("--") && argument.contains('f'))
}

fn push(rest: &[String], safe_push: bool) -> Result<(), String> {
    if rest.iter().any(|s| forced(s)) {
        return Err(
            "git push force/mirror — history rewrites are deliberate + manual, never automated"
                .into(),
        );
    }
    if !safe_push && !rest.iter().any(|s| s == "--delete" || s.starts_with(':')) {
        return Err("raw 'git push' — house policy: use safe-push (gitleaks-scans the outgoing commits, then pushes)".into());
    }
    Ok(())
}

pub(super) fn deletion(rest: &[String]) -> (String, Vec<String>) {
    let delete = rest.iter().any(|s| s == "--delete");
    let mut repository = None;
    let mut positional = Vec::new();
    let mut arguments = rest.iter().map(|s| strip(s));
    while let Some(argument) = arguments.next() {
        if let Some(named) = argument.strip_prefix("--repo=") {
            repository = Some(named);
        } else if argument == "--repo" {
            repository = arguments.next();
        } else if ["-o", "--push-option", "--receive-pack", "--exec"].contains(&argument) {
            arguments.next();
        } else if !argument.starts_with('-') {
            positional.push(argument);
        }
    }
    let mut positional = positional.into_iter();
    let remote = repository.or_else(|| positional.next()).unwrap_or("origin").to_owned();
    let branches = positional
        .filter_map(|refspec| {
            let name = if delete { refspec } else { refspec.strip_prefix(':')? };
            Some(name.strip_prefix("refs/heads/").unwrap_or(name).to_owned())
        })
        .collect();
    (remote, branches)
}

impl<'a> State<'a> {
    fn queued_deletion(&self, rest: &[String], directory: &str, cwd: &str) -> Result<(), String> {
        let root = if directory.is_empty() { Some(if cwd.is_empty() { self.cwd.clone() } else { cwd.to_owned() }) } else { self.resolve(directory, cwd) };
        let Some(root) = root else { return Ok(()) };
        let (remote, branches) = deletion(rest);
        match (self.hold)(self.deadline, &root, &remote, &branches) {
            Some(hold) => Err(format!("git push deleting a land branch with an Autoland run in flight: {hold}")),
            None => Ok(()),
        }
    }
    pub(super) fn git(&mut self, args: &[String], cwd: &str) -> Result<(), String> {
        let (directory, i) = global_options(args);
        let Some(sub) = args.get(i) else {
            return Ok(());
        };
        let rest = &args[i + 1..];
        match sub.as_str() {
            "worktree" if rest.first().is_some_and(|s| s == "add") => {
                self.worktree_add(&rest[1..], directory, cwd);
                Ok(())
            }
            "push" => push(rest, self.safe_push).and_then(|()| if self.safe_push { Ok(()) } else { self.queued_deletion(rest, directory, cwd) }),
            "clean" => self.clean(rest, directory, cwd),
            _ => Ok(()),
        }
    }
    fn worktree_add(&mut self, rest: &[String], directory: &str, cwd: &str) {
        let root = if directory.is_empty() {
            if cwd.is_empty() {
                self.cwd.clone()
            } else {
                cwd.to_owned()
            }
        } else {
            let Some(p) = self.resolve(directory, cwd) else {
                return;
            };
            p
        };
        let mut skip = false;
        for t in rest {
            if skip {
                skip = false;
                continue;
            }
            if ["-b", "-B", "--reason"].contains(&t.as_str()) {
                skip = true;
            } else if !t.starts_with('-') {
                if let Some(p) = self.resolve(t, &root) {
                    self.created.push(p);
                }
                break;
            }
        }
    }
    fn clean(&mut self, rest: &[String], directory: &str, cwd: &str) -> Result<(), String> {
        if !rest
            .iter()
            .any(|s| s == "--force" || (s.starts_with('-') && s.contains('f')))
        {
            return Ok(());
        }
        self.delete_seen = true;
        let p = if directory.is_empty() {
            lexical(if cwd.is_empty() { &self.cwd } else { cwd })
        } else {
            let Some(p) = self.resolve(directory, cwd) else {
                self.unread = directory.into();
                return Ok(());
            };
            p
        };
        self.clean_target(&p)
    }
    fn clean_target(&self, p: &str) -> Result<(), String> {
        if self.never(p) {
            return Err(format!("git clean -f in '{p}' — never"));
        }
        if let Some(why) = self.owner_reason(p) {
            return Err(format!("git clean -f: {why}"));
        }
        if git(&self.cwd, &["rev-parse", "--show-toplevel"])
            .is_some_and(|r| !r.is_empty() && under(p, &r))
            || self.disposable(p)
            || self.lane(p).is_some_and(|w| self.strong(&w))
        {
            return Ok(());
        }
        Err(format!(
            "git clean -f in '{p}' — outside this session's repo, and untracked files there are not in any object database; {OVERRIDE}"
        ))
    }
    pub(super) fn git_write(
        &self,
        verb: &str,
        method: &str,
        endpoint: &str,
        mutation: bool,
    ) -> Result<(), String> {
        if self.safe_push {
            return Ok(());
        }
        if !["GET", "HEAD"].contains(&method.to_uppercase().as_str()) && GIT_DATA.is_match(endpoint)
        {
            return Err(format!(
                "{verb} write to the GitHub git data or contents API — it makes commits or moves refs around safe-push; use safe-push"
            ));
        }
        if mutation
            && Regex::new(r"(^|/)graphql([?]|$)")
                .unwrap()
                .is_match(endpoint)
        {
            return Err(format!(
                "{verb} GraphQL ref mutation — it makes commits or moves refs around safe-push; use safe-push"
            ));
        }
        Ok(())
    }
    fn gh_body(&self, value: &str, input: bool) -> bool {
        let mutation = !input && MUTATION.is_match(value);
        let path = if input {
            value
        } else if let Some((_, p)) = value.split_once("=@") {
            p
        } else if let Some(p) = value.strip_prefix('@') {
            p
        } else {
            return mutation;
        };
        if path == "-" {
            return true;
        }
        let Some(p) = self.resolve(path, "") else {
            return true;
        };
        match std::fs::read_to_string(p) {
            Ok(text) => mutation || MUTATION.is_match(text.split('\0').next().unwrap_or("")),
            Err(_) => true,
        }
    }
    fn gh_option<'s>(&self, prev: &str, s: &'s str, request: &mut Request<'s>) -> bool {
        match prev {
            "-X" | "--method" => request.method = s,
            "-f" | "-F" | "--field" | "--raw-field" => {
                request.body = true;
                request.mutation |= self.gh_body(s, false);
            }
            "--input" => {
                request.body = true;
                request.mutation |= self.gh_body(s, true);
            }
            "-H" | "--header" | "-q" | "--jq" | "-t" | "--template" | "--hostname" | "--cache"
            | "-p" | "--preview" => {}
            _ => return false,
        }
        true
    }
    fn gh_word<'s>(&self, s: &'s str, request: &mut Request<'s>) {
        if let Some(m) = s
            .strip_prefix("-X")
            .filter(|s| !s.is_empty())
            .or_else(|| s.strip_prefix("--method="))
        {
            request.method = m;
        } else if (s.starts_with("-f") || s.starts_with("-F")) && s.len() > 2 {
            request.body = true;
            request.mutation |= self.gh_body(&s[2..], false);
        } else if s.starts_with("--field=") || s.starts_with("--raw-field=") {
            request.body = true;
            request.mutation |= self.gh_body(s.split_once('=').unwrap().1, false);
        } else if let Some(p) = s.strip_prefix("--input=") {
            request.body = true;
            request.mutation |= self.gh_body(p, true);
        } else if !s.starts_with('-') && request.target.is_empty() {
            request.target = s;
        }
    }
    pub(super) fn gh(&self, args: &[String]) -> Result<(), String> {
        if args.first().map(String::as_str) != Some("api") {
            return Ok(());
        }
        let mut request = Request::default();
        let mut prev = "";
        for t in &args[1..] {
            let s = strip(t);
            prev = if self.gh_option(prev, s, &mut request) {
                ""
            } else {
                self.gh_word(s, &mut request);
                s
            };
        }
        self.git_write("gh api", request.method(), request.target, request.mutation)
    }
}
