use super::super::failure::{Failure, failed};
use super::super::remote::output;
use std::process::Command;
use std::time::Duration;

const UNIT: &str = "north-stable-hold.service";
const STEP: Duration = Duration::from_secs(60);

pub(super) struct Released(bool);

fn systemctl(step: &str, args: &[&str]) -> Result<String, Failure> {
    output("this host", step, Command::new("systemctl").arg("--user").args(args), STEP)
}

fn active() -> Result<bool, Failure> {
    let state = systemctl("systemctl show", &["show", "--property=ActiveState", "--value", UNIT]).map_err(|failure| {
        failed(1, format!("this shell cannot reach the systemd user manager, so it cannot release {UNIT}; run `north runtime order` as the coordinator's step from the coordinator's shell ({})", failure.message))
    })?;
    Ok(state.trim() == "active")
}

/// Stops the unit that holds the promotion lock against the lander's after-land promotion, so the ordered promotion can take it; the pin file stays, since it keeps the follow timers from promoting origin/main past the ordered commit meanwhile.
pub(super) fn release() -> Result<Released, Failure> {
    let was = active()?;
    if was {
        systemctl("systemctl stop", &["stop", UNIT])?;
        println!("north runtime order: stopped {UNIT} for the promotion; it is started again after");
    }
    Ok(Released(was))
}

pub(super) fn restore(released: Released) -> Result<(), Failure> {
    if !released.0 {
        return Ok(());
    }
    systemctl("systemctl start", &["start", UNIT])?;
    match active()? {
        true => {
            println!("north runtime order: {UNIT} is holding the promotion lock again");
            Ok(())
        }
        false => Err(failed(1, format!("{UNIT} did not start again: its condition, the stable pin file, is missing here"))),
    }
}
