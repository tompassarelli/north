mod deploy;
pub(crate) mod layout;
mod update;

pub(crate) fn run(arguments: &[String]) -> u8 {
    let layout = layout::Layout::from_env();
    let result = match arguments.split_first() {
        Some((command, [])) if command == "update" => update::update(&layout),
        Some((command, hosts)) if command == "deploy" && !hosts.is_empty() => deploy::deploy(&layout, hosts),
        _ => {
            eprintln!("usage: north runtime update | north runtime deploy HOST...");
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
