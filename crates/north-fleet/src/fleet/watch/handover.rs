//! What a turn watch carries into its session host's next image (docs/rationale/runtime-follow.md): the turns it is
//! tracking, the replies it has read, the quiet children it may still tell parents about, its checkpoints and the
//! prompts the provider has not accepted yet.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{TurnWatch, View};
use crate::fleet::checkpoint::Checkpoints;
use crate::fleet::model::Entry;
use crate::fleet::silent::Quiet;
use crate::fleet::turns::Turns;
use crate::fleet::unaccepted;

#[derive(Serialize, Deserialize)]
struct Carried {
    turns: Turns,
    views: Vec<CarriedView>,
    finished: HashMap<String, bool>,
    rolled_over: bool,
    checkpoints: Checkpoints,
    compacted: HashMap<String, HashSet<String>>,
    quiet: Vec<CarriedQuiet>,
    unaccepted: Vec<unaccepted::Entry>,
}

#[derive(Serialize, Deserialize)]
struct CarriedView {
    conversation: String,
    reply: Option<String>,
    lines: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct CarriedQuiet {
    conversation: String,
    turn: u64,
    reply: String,
    checked: bool,
    idled: bool,
    completed: bool,
    quiet_ms: u64,
    after_ms: u64,
    eta_min: Option<u64>,
    lines: Vec<Entry>,
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl Carried {
    /// Waits for the watch's parent checks to settle, then reads what it carries.
    fn of(watch: &mut TurnWatch) -> Self {
        watch.finish_probes();
        let views = watch.views.iter().map(|(conversation, view)| CarriedView { conversation: conversation.clone(), reply: view.reply.clone(), lines: view.transcript.entries.clone() }).collect();
        let quiet = watch
            .quiet
            .iter()
            .map(|(conversation, quiet)| CarriedQuiet {
                conversation: conversation.clone(),
                turn: quiet.turn,
                reply: quiet.reply.clone(),
                checked: quiet.checked,
                idled: quiet.idled,
                completed: quiet.completed,
                quiet_ms: millis(quiet.since.elapsed()),
                after_ms: millis(quiet.after),
                eta_min: quiet.eta_min,
                lines: quiet.lines.clone(),
            })
            .collect();
        Self {
            turns: watch.turns.clone(),
            views,
            finished: watch.finished.clone(),
            rolled_over: watch.rolled_over,
            checkpoints: watch.checkpoints.clone(),
            compacted: watch.compacted.clone(),
            quiet,
            unaccepted: watch.unaccepted.journal().held().to_vec(),
        }
    }

    fn restore(self, watch: &mut TurnWatch) {
        watch.turns = self.turns;
        watch.views = self
            .views
            .into_iter()
            .map(|carried| {
                let mut view = View::of(&carried.conversation);
                view.reply = carried.reply;
                view.transcript.entries = carried.lines;
                (carried.conversation, view)
            })
            .collect();
        watch.finished = self.finished;
        watch.rolled_over = self.rolled_over;
        watch.checkpoints = self.checkpoints;
        watch.compacted = self.compacted;
        watch.quiet = self
            .quiet
            .into_iter()
            .map(|carried| {
                let since = Instant::now().checked_sub(Duration::from_millis(carried.quiet_ms)).unwrap_or_else(Instant::now);
                let quiet = Quiet { turn: carried.turn, reply: carried.reply, checked: carried.checked, idled: carried.idled, probe: None, cancelled: Arc::default(), completed: carried.completed, since, after: Duration::from_millis(carried.after_ms), eta_min: carried.eta_min, lines: carried.lines };
                (carried.conversation, quiet)
            })
            .collect();
        watch.unaccepted.journal().hold(self.unaccepted);
    }
}

pub(super) fn hand_over(watch: &mut TurnWatch) -> serde_json::Value {
    serde_json::to_value(Carried::of(watch)).unwrap_or(serde_json::Value::Null)
}

pub(super) fn take_over(watch: &mut TurnWatch, state: serde_json::Value) {
    match serde_json::from_value::<Carried>(state) {
        Ok(carried) => carried.restore(watch),
        Err(error) => eprintln!("north session-host: the watch its previous image handed over was not read: {error}"),
    }
}
