use std::path::PathBuf;

const SHADOW_MINUTES: u64 = 15;

pub(super) struct Options {
    pub(super) dry_run: bool,
    pub(super) shadow_minutes: u64,
    pub(super) repos: Vec<PathBuf>,
    pub(super) checker: String,
    pub(super) judge: Option<PathBuf>,
    pub(super) held: Option<PathBuf>,
}

pub(super) fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { dry_run: false, shadow_minutes: SHADOW_MINUTES, repos: Vec::new(), checker: "safe-push".to_owned(), judge: None, held: None };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--dry-run" => options.dry_run = true,
            flag => set(&mut options, flag, rest.next()?)?,
        }
    }
    if options.repos.is_empty() {
        options.repos.push(PathBuf::from("."));
    }
    (options.held.is_none() || options.judge.is_some()).then_some(options)
}

fn set(options: &mut Options, flag: &str, value: &str) -> Option<()> {
    match flag {
        "--judge" => options.judge = Some(PathBuf::from(value)),
        "--held" => options.held = Some(PathBuf::from(value)),
        "--shadow-minutes" => options.shadow_minutes = value.parse().ok()?,
        "--repo" => options.repos.push(PathBuf::from(value)),
        "--checker" => value.clone_into(&mut options.checker),
        _ => return None,
    }
    Some(())
}
