mod desk;
mod digest;
mod digest_view;
mod editor;
mod form;
mod order;
mod painter;
mod table;
mod runner;
mod screen;
mod slash;
mod state_clock;
mod view;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod frame_tests;

use std::env;
use std::io::{self, Stdout, Write};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Transcript;
use crate::fleet::registry::Fleet;

use desk::{FleetDesk, SessionDesk};
use digest::SessionFacts;
use painter::TranscriptPainter;
use runner::Runner;
use screen::{Command, Screen};

const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub fn run(fleet: Fleet) -> NorthResult<()> {
    let mut desk = FleetDesk::new(fleet);
    let mut screen = Screen::new(env::current_dir()?, desk.accounts());
    let (_guard, mut terminal) = TerminalGuard::enter()?;
    drive(&mut desk, &mut screen, &mut terminal, &mut terminal_events)
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> NorthResult<(Self, Terminal<CrosstermBackend<Stdout>>)> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        let _ = execute!(stdout, PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        Ok((Self, Terminal::new(CrosstermBackend::new(stdout))?))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags, DisableBracketedPaste, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

type EventSource<'a> = dyn FnMut(Duration) -> NorthResult<Option<Event>> + 'a;

fn terminal_events(timeout: Duration) -> NorthResult<Option<Event>> {
    Ok(if event::poll(timeout)? { Some(event::read()?) } else { None })
}

fn drive<D: SessionDesk, B: Backend>(desk: &mut D, screen: &mut Screen, terminal: &mut Terminal<B>, events: &mut EventSource<'_>) -> NorthResult<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let mut runner = Runner::new(desk.actions());
    let mut painter = TranscriptPainter::default();
    let mut next_poll = Instant::now();
    let mut dirty = true;
    let mut minute = 0;
    while !screen.quitting() {
        if settle_completed(&mut runner, screen) {
            dirty = true;
            next_poll = Instant::now();
        }
        if Instant::now() >= next_poll {
            dirty |= refresh(desk, screen) | !screen.busy.is_empty();
            next_poll = Instant::now() + POLL_INTERVAL;
        }
        let now = now_ms();
        dirty |= std::mem::replace(&mut minute, now / 60_000) != now / 60_000;
        if dirty {
            terminal.draw(|frame| view::draw(frame, screen, screen_transcript(desk, screen), &mut painter, now)).map_err(|error| NorthError::Io(io::Error::other(error)))?;
            ring_bell(screen);
            dirty = false;
        }
        if let Some(event) = events(next_poll.saturating_duration_since(Instant::now()))? {
            dirty = true;
            if let Some(command) = apply_event(screen, event) {
                screen.begin(&command, desk.transcript(command.session()));
                runner.start(command);
            }
        }
    }
    if !runner.idle() {
        screen.notice = Some(screen::Notice::Info("finishing pending commands before quitting…".into()));
        terminal.draw(|frame| view::draw(frame, screen, screen_transcript(desk, screen), &mut painter, now_ms())).map_err(|error| NorthError::Io(io::Error::other(error)))?;
        for completion in runner.drain() {
            screen.settle(completion.command, completion.outcome);
        }
    }
    Ok(())
}

fn settle_completed(runner: &mut Runner, screen: &mut Screen) -> bool {
    let completions = runner.completed();
    let settled = !completions.is_empty();
    for completion in completions {
        screen.settle(completion.command, completion.outcome);
    }
    settled
}


fn screen_transcript<'d, D: SessionDesk>(desk: &'d D, screen: &Screen) -> Option<&'d Transcript> {
    desk.transcript(screen.selected.as_deref()?)
}

fn refresh<D: SessionDesk>(desk: &mut D, screen: &mut Screen) -> bool {
    match desk.refresh() {
        Ok(false) => false,
        Ok(true) => {
            screen.sync(desk.sessions());
            screen.reconcile(|name| desk.transcript(name));
            let facts = screen.sessions.iter().map(|session| (session.record.name.clone(), SessionFacts::of(&session.record, desk.transcript(&session.record.name)))).collect();
            screen.observe(desk.digest(), facts);
            screen.digest.wisp_payoff = desk.wisp_payoff();
            true
        }
        Err(error) => {
            screen.fail(&error);
            true
        }
    }
}

fn apply_event(screen: &mut Screen, event: Event) -> Option<Command> {
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => screen.handle_key(key),
        Event::Paste(text) => {
            screen.paste(&text);
            None
        }
        _ => None,
    }
}

fn ring_bell(screen: &mut Screen) {
    if screen.take_bell() {
        let mut stdout = io::stdout();
        let _ = stdout.write_all(b"\x07").and_then(|()| stdout.flush());
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}
