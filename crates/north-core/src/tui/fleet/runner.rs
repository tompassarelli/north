use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use crate::error::NorthResult;
use crate::fleet::prompt::deepened;

use super::desk::SessionActions;
use super::screen::Command;

pub struct Completion {
    pub command: Command,
    pub outcome: NorthResult<()>,
}

pub struct Runner {
    actions: Arc<dyn SessionActions>,
    sender: Sender<Completion>,
    receiver: Receiver<Completion>,
    queued: BTreeMap<String, VecDeque<Command>>,
}

impl Runner {
    pub fn new(actions: Arc<dyn SessionActions>) -> Self {
        let (sender, receiver) = channel();
        Self { actions, sender, receiver, queued: BTreeMap::new() }
    }

    pub fn start(&mut self, command: Command) {
        match self.queued.get_mut(command.session()) {
            Some(waiting) => waiting.push_back(command),
            None => {
                self.queued.insert(command.session().to_owned(), VecDeque::new());
                self.spawn(command);
            }
        }
    }

    pub fn idle(&self) -> bool {
        self.queued.is_empty()
    }

    pub fn drain(&mut self) -> Vec<Completion> {
        let mut completions = Vec::new();
        while !self.idle() {
            let Ok(completion) = self.receiver.recv() else { break };
            self.advance(completion.command.session());
            completions.push(completion);
        }
        completions
    }

    pub fn completed(&mut self) -> Vec<Completion> {
        let completions: Vec<Completion> = self.receiver.try_iter().collect();
        for completion in &completions {
            self.advance(completion.command.session());
        }
        completions
    }

    fn advance(&mut self, session: &str) {
        let next = self.queued.get_mut(session).and_then(VecDeque::pop_front);
        match next {
            Some(command) => self.spawn(command),
            None => {
                self.queued.remove(session);
            }
        }
    }

    fn spawn(&self, command: Command) {
        let actions = Arc::clone(&self.actions);
        let sender = self.sender.clone();
        thread::spawn(move || {
            let outcome = perform(actions.as_ref(), &command);
            let _ = sender.send(Completion { command, outcome });
        });
    }
}

fn perform(actions: &dyn SessionActions, command: &Command) -> NorthResult<()> {
    match command {
        Command::Launch(request) => actions.launch(request),
        Command::Send { name, text, delivery, deepen: true } => actions.send(name, &deepened(text), *delivery),
        Command::Send { name, text, delivery, deepen: false } => actions.send(name, text, *delivery),
        Command::Revive(name) => actions.revive(name),
        Command::Interrupt(name) => actions.interrupt(name),
        Command::Stop(name) => actions.stop(name),
        Command::Forget(name) => actions.forget(name),
        Command::Decide { gate, decision, .. } => actions.decide(*gate, decision.clone()),
    }
}
