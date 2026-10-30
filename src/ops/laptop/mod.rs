mod ctx;
mod die;
mod observe;
mod stamp;
mod state;
mod tick;
mod words;
mod zone;
#[cfg(test)]
mod tests;

pub(crate) use ctx::{Ctx, hostname};
pub(crate) use die::Die;
pub(crate) use zone::Zone;

const USAGE: &str = "north laptop open|close|drain|toggle|status|tick|observe: the laptop's placement availability, one state on the ledger host.

open [until HH:MM] [REASON...]   available: laptop-runner takes laptop and pool jobs
close [until HH:MM] [REASON...]  draining with a 2-minute grace, then unavailable
drain [until HH:MM] [REASON...]  draining with a 15-minute grace, then unavailable
toggle                           close when available, else open
status [--json]                  one line: state, since, until, reason, by, running and queued laptop jobs
tick                             the window timer: apply window edges and expired words, settle draining
observe JSON                     apply declared-gaming, seat-idle, sleep and low-battery observations from laptop-auto

A word holds until its `until`, else until the next window edge. Draining becomes unavailable once no laptop job
is running. Where ~/.config/threads/remote names the ledger host, every command runs there over ssh.
";

pub(crate) fn run(arguments: &[String]) -> u8 {
    finish(Ctx::from_env().main(arguments))
}

pub(crate) fn finish(result: Result<String, Die>) -> u8 {
    match result {
        Ok(out) => {
            print!("{out}");
            0
        }
        Err(die) => {
            if !die.message.is_empty() {
                eprintln!("laptop: {}", die.message);
            }
            die.code
        }
    }
}

impl Ctx {
    pub(crate) fn main(&self, argv: &[String]) -> Result<String, Die> {
        let verb = argv.first().map(String::as_str);
        if matches!(verb, None | Some("-h" | "--help" | "help")) {
            return Ok(USAGE.to_owned());
        }
        let verb = verb.unwrap_or_default();
        if self.forwarding() {
            return self.forward(argv);
        }
        let _lock = self.lock()?;
        let args = &argv[1..];
        match verb {
            "toggle" if !args.is_empty() => Err(Die::new(2, "toggle takes no arguments")),
            "open" | "close" | "drain" | "toggle" => self.word(verb, args),
            "status" => self.status(args),
            "tick" if args.is_empty() => self.tick(),
            "observe" => self.observe(args),
            _ => Err(Die::new(2, format!("unknown command {verb}; see north laptop --help"))),
        }
    }
}
