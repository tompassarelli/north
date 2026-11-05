const HEAD_NORTH: &str = "./bin/head-north";
const NO_BUILD: &str = "--no-build";

fn builds(words: &[&str]) -> Option<usize> {
    let at = words.iter().position(|word| *word == HEAD_NORTH)?;
    let flag = if words.get(at + 1) == Some(&"--code") { at + 3 } else { at + 1 };
    (words.get(flag) != Some(&NO_BUILD)).then_some(at)
}

pub fn runs_head_north(command: &str) -> bool {
    builds(&command.split_whitespace().collect::<Vec<_>>()).is_some()
}

pub fn command(commands: &[String]) -> Option<String> {
    let (mut environment, mut code, mut any) = (None, Vec::new(), false);
    for command in commands {
        let words: Vec<&str> = command.split_whitespace().collect();
        let Some(at) = builds(&words) else { continue };
        environment.get_or_insert_with(|| words[..at].iter().filter(|word| word.contains('=')).map(|word| format!("{word} ")).collect::<String>());
        match words.get(at + 1..at + 3) {
            Some(["--code", paths]) => code.extend(paths.split(',').filter(|path| !path.is_empty())),
            _ => any = true,
        }
    }
    code.sort_unstable();
    code.dedup();
    let code = if any { String::new() } else { format!(" --code {}", code.join(",")) };
    environment.map(|environment| format!("{environment}{HEAD_NORTH}{code} --build"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Phases {
    pub others: Vec<String>,
    pub build: String,
    pub head_north: Vec<String>,
}

pub fn phases(commands: Vec<String>) -> Result<Phases, Vec<String>> {
    let Some(build) = command(&commands) else { return Err(commands) };
    let (head_north, others): (Vec<String>, Vec<String>) = commands.into_iter().partition(|command| runs_head_north(command));
    Ok(Phases { others, build, head_north })
}

#[cfg(test)]
#[path = "shared_build_tests.rs"]
mod tests;
