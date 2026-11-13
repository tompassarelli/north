use super::super::failure::{Failure, failed};
use super::super::layout::Layout;
use super::PIN;
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

/// With runtime_promotion = landing every later landing promotes on its own, so the order leaves the unit stopped and clears the hand pin it held the promotion lock for; says the unit was not started again.
pub(super) fn lift(layout: &Layout) -> Result<bool, Failure> {
    let pin = layout.state.join(PIN);
    match std::fs::remove_file(&pin) {
        Ok(()) => println!("north runtime order: cleared the hand pin {} and left {UNIT} stopped; every later landing that changes a runtime input promotes on its own", pin.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failed(1, format!("the hand pin {} was not cleared: {error}", pin.display()))),
    }
    Ok(false)
}

/// Says whether the unit was started again, which queues it on the promotion lock the order still holds.
pub(super) fn restore(released: Released) -> Result<bool, Failure> {
    if !released.0 {
        return Ok(false);
    }
    systemctl("systemctl start", &["start", UNIT])?;
    match active()? {
        true => {
            println!("north runtime order: {UNIT} is started again and takes the promotion lock as the order lets go of it");
            Ok(true)
        }
        false => Err(failed(1, format!("{UNIT} did not start again: its condition, the stable pin file, is missing here"))),
    }
}
