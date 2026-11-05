const HEAD_NORTH: &str = "./bin/head-north";

pub fn runs_head_north(command: &str) -> bool {
    command.split_whitespace().any(|word| word == HEAD_NORTH)
}

pub fn command(commands: &[String]) -> Option<String> {
    let (mut environment, mut code, mut any) = (None, Vec::new(), false);
    for command in commands {
        let words: Vec<&str> = command.split_whitespace().collect();
        let Some(at) = words.iter().position(|word| *word == HEAD_NORTH) else { continue };
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
