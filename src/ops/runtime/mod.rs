mod deploy;
mod failure;
pub(crate) mod layout;
mod store;
mod update;

const USAGE: &str = "usage: north runtime update | north runtime deploy HOST...";

pub(crate) fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let layout = layout::Layout::from_env();
    let result = match arguments.split_first() {
        Some((command, [])) if command == "update" => update::update(&layout),
        Some((command, hosts)) if command == "deploy" && !hosts.is_empty() => {
            if let Some(flag) = hosts.iter().find(|host| host.starts_with('-')) {
                eprintln!("north runtime deploy: unknown flag {flag}\n{USAGE}");
                return 64;
            }
            deploy::deploy(&layout, hosts)
        }
        _ => {
            eprintln!("{USAGE}");
            return 64;
        }
    };
    match result {
        Ok(()) => 0,
        Err(failure) => {
            eprintln!("north runtime {}: {}", arguments[0], failure.message);
            failure.code
        }
    }
}
