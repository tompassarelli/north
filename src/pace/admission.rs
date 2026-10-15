use super::control::Control;
use super::target::Target;
use super::window::{MAX_AGE_SECONDS, Window};

const MIN_INTERVAL_SECONDS: f64 = 60.0;
const SLOW_READ_SECONDS: f64 = 20.0;

pub fn is_fresh(control: &Control, at: f64) -> bool {
    (0.0..=MAX_AGE_SECONDS).contains(&(at - control.at.unwrap_or(0.0)))
        && !control.windows.is_empty()
        && control.windows.iter().all(|w| (0.0..=MAX_AGE_SECONDS).contains(&w.age(at)))
}

pub fn interval(previous: f64, elapsed: f64, throttled: bool, retry_after: f64) -> f64 {
    if throttled {
        return MIN_INTERVAL_SECONDS.max(previous * 2.0).max(retry_after);
    }
    if elapsed > SLOW_READ_SECONDS { MIN_INTERVAL_SECONDS.max((elapsed * 2.0).ceil()) } else { MIN_INTERVAL_SECONDS }
}

fn projection_hold(account: &str, w: &Window, target: Option<&Target>, now: f64) -> Option<String> {
    let projection = w.projected(target, now)?;
    (projection > 100.0).then(|| {
        format!(
            "Hold: {account} {} used {:.1}% + burn {:.2}%/h x {:.2}h to {} = {projection:.1}% > 100%.",
            w.window,
            w.used_pct,
            w.projection_burn(target, now).unwrap_or(0.0),
            w.horizon(target, now),
            target.map_or("reset", |t| t.by.as_str()),
        )
    })
}

fn window_hold(account: &str, w: &Window, target: Option<&Target>, now: f64) -> Option<String> {
    if w.is_week() && w.used_pct >= w.ceiling(target) {
        return Some(format!(
            "Hold: {account} week used {:.1}% reaches the {}% weekly drain ceiling; wait for reset.",
            w.used_pct,
            w.ceiling(target)
        ));
    }
    projection_hold(account, w, target, now)
}

pub fn refusal(account: &str, control: &Control, now: f64, target: Result<Option<Target>, String>) -> Option<String> {
    let target = match target {
        Ok(target) => target,
        Err(error) => return Some(format!("Hold: {error}.")),
    };
    if !(0.0..=MAX_AGE_SECONDS).contains(&(now - control.at.unwrap_or(0.0))) {
        return Some(format!("Hold: {account} pace sample is not fresh (needs <=120s); refresh before spawning."));
    }
    for w in &control.windows {
        let age = w.age(now);
        if !(0.0..=MAX_AGE_SECONDS).contains(&age) {
            return Some(format!("Hold: {account} {} usage is {age:.0}s old; needs <=120s after refresh.", w.window));
        }
    }
    if let Some(hold) = control.windows.iter().find_map(|w| window_hold(account, w, target.as_ref(), now)) {
        return Some(hold);
    }
    let allowed = control.allowed_workers.unwrap_or(0);
    let Some(workers) = control.workers.filter(|_| control.held != Some(true)) else {
        return Some(format!("Hold: {account} fresh usage or live worker attribution is unavailable; allowed {allowed}."));
    };
    (workers >= allowed).then(|| format!("Hold: {account} workers {workers}/{allowed} allowed by measured pace."))
}
