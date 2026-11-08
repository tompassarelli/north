use std::path::PathBuf;

pub struct Options {
    pub enforce: Option<PathBuf>,
    pub record: Option<PathBuf>,
    pub except: Vec<String>,
    pub tree: Option<PathBuf>,
    pub scratch: Option<PathBuf>,
    pub trace: bool,
    pub command: Vec<String>,
}

impl Options {
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Self { enforce: None, record: None, except: Vec::new(), tree: None, scratch: None, trace: false, command: Vec::new() };
        let mut rest = arguments.iter();
        while let Some(argument) = rest.next() {
            match argument.as_str() {
                "--enforce" => options.enforce = Some(take(&mut rest, argument)?.into()),
                "--record" => options.record = Some(take(&mut rest, argument)?.into()),
                "--except" => options.except.push(take(&mut rest, argument)?.clone()),
                "--tree" => options.tree = Some(take(&mut rest, argument)?.into()),
                "--scratch" => options.scratch = Some(take(&mut rest, argument)?.into()),
                "--trace" => options.trace = true,
                "--" => {
                    options.command = rest.cloned().collect();
                    break;
                }
                other => return Err(format!("unknown argument {other}")),
            }
        }
        if options.command.is_empty() {
            return Err("no command after --".into());
        }
        Ok(options)
    }

    pub fn record_file(&self) -> Option<PathBuf> {
        let record = self.record.as_ref()?;
        if !record.is_dir() {
            return Some(record.clone());
        }
        let unit = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let (binary, test) = (unit("NEXTEST_BINARY_ID")?, unit("NEXTEST_TEST_NAME")?);
        let excepted = self.except.iter().any(|name| binary == *name || binary.strip_prefix(name.as_str()).is_some_and(|rest| rest.starts_with("::")));
        (!excepted).then(|| record.join(north_base::verdict_cache::fingerprint::record_name(&binary, &test)))
    }
}

fn take<'a>(rest: &mut std::slice::Iter<'a, String>, argument: &str) -> Result<&'a String, String> {
    rest.next().ok_or_else(|| format!("{argument} needs a value"))
}
