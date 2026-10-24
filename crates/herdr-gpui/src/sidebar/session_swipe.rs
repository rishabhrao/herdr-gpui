//! A sideways swipe on the sidebar steps to the next or previous running
//! local session, as Arc's sidebar steps between profiles: fingers moving
//! left go to the next session, right to the one before. A swipe moves one
//! session however far it travels or coasts, and the ends do not wrap.
use crate::{HerdrWindow, window::Flash};
use gpui::{App, Context, ScrollWheelEvent, Window, px};
use std::time::{Duration, Instant};

/// Sideways travel, in pixels, that makes a swipe.
const TRAVEL: f32 = 90.;
/// A pause this long ends a swipe. A trackpad's or a fling's coasting keeps
/// one going, so it steps once.
const GAP: Duration = Duration::from_millis(250);
/// How long a swipe made before the session list was read still counts.
const WAIT: Duration = Duration::from_secs(2);
/// A scroll counts as sideways when it moves this many times more across
/// than down; anything else scrolls the list.
const SIDEWAYS: f32 = 2.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Previous,
    Next,
}

/// One swipe's progress, and a step waiting for the session list.
#[derive(Debug, Default)]
pub(crate) struct SessionSwipe {
    travel: f32,
    last: Option<Instant>,
    stepped: bool,
    waiting: Option<(Step, Instant)>,
}

impl SessionSwipe {
    /// Adds one scroll's travel; the step to take when it completes a swipe.
    pub(crate) fn scroll(&mut self, (x, y): (f32, f32), now: Instant) -> Option<Step> {
        if x.abs() < y.abs() * SIDEWAYS || x == 0. {
            return None;
        }
        if self.last.is_none_or(|last| now.duration_since(last) > GAP) {
            self.travel = 0.;
            self.stepped = false;
        }
        self.last = Some(now);
        self.travel += x;
        if self.stepped || self.travel.abs() < TRAVEL {
            return None;
        }
        self.stepped = true;
        Some(if self.travel < 0. {
            Step::Next
        } else {
            Step::Previous
        })
    }

    /// Whether a step waits for the session list, so it should be read.
    pub(crate) fn waiting(&self, now: Instant) -> bool {
        self.waiting
            .is_some_and(|(_, at)| now.duration_since(at) < WAIT)
    }

    fn take_waiting(&mut self, now: Instant) -> Option<Step> {
        self.waiting
            .take()
            .filter(|(_, at)| now.duration_since(*at) < WAIT)
            .map(|(step, _)| step)
    }
}

/// The session `step` moves to from `current` among `sessions`.
pub(crate) fn neighbour(sessions: &[&str], current: Option<usize>, step: Step) -> Option<usize> {
    let current = current?;
    match step {
        Step::Previous => current.checked_sub(1),
        Step::Next => Some(current + 1).filter(|next| *next < sessions.len()),
    }
}

/// The sessions as dots, the one shown filled.
fn dots(count: usize, shown: usize) -> String {
    (0..count)
        .map(|index| if index == shown { "●" } else { "○" })
        .collect::<Vec<_>>()
        .join(" ")
}

impl HerdrWindow {
    /// The sidebar's listener for sideways swipes.
    pub(super) fn session_swipe_listener(
        &self,
        cx: &mut Context<Self>,
    ) -> impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static {
        cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
            // Shift turns a mouse wheel sideways; that is not a swipe.
            if event.modifiers.shift {
                return;
            }
            let delta = event.delta.pixel_delta(px(20.));
            let travel = (f32::from(delta.x), f32::from(delta.y));
            if let Some(step) = this.session_swipe.scroll(travel, Instant::now()) {
                this.step_session(step, true, cx);
            }
        })
    }

    /// A step that waited for the session list, now that it was read.
    pub(crate) fn step_waiting_session(&mut self, cx: &mut Context<Self>) {
        if self.sessions.scanning {
            return;
        }
        if let Some(step) = self.session_swipe.take_waiting(Instant::now()) {
            self.step_session(step, false, cx);
        }
    }

    /// Attaches to the running local session `step` away, and says which.
    /// `wait` lets a list not read yet be read first.
    fn step_session(&mut self, step: Step, wait: bool, cx: &mut Context<Self>) {
        // Swipes move between this machine's sessions, from one of them.
        if self.selected_endpoint != 0 {
            return;
        }
        if self.sessions.entries.is_empty() && wait {
            self.session_swipe.waiting = Some((step, Instant::now()));
            self.sessions.refresh();
            cx.notify();
            return;
        }
        let socket = self.local_socket();
        let running: Vec<_> = self
            .sessions
            .entries
            .iter()
            .filter(|session| session.state.running())
            .collect();
        let names: Vec<&str> = running
            .iter()
            .map(|session| session.name.as_str())
            .collect();
        let current = running
            .iter()
            .position(|session| socket.as_ref() == Some(&session.socket));
        let Some(next) = neighbour(&names, current, step) else {
            if let Some(current) = current {
                let flash = format!("{}   {}", names[current], dots(names.len(), current));
                self.show_flash(Flash::hint(flash), cx);
            }
            return;
        };
        let name = names[next].to_owned();
        let flash = Flash::hint(format!("{name}   {}", dots(names.len(), next)));
        self.attach_local_session(&name, cx);
        self.show_flash(flash, cx);
    }
}

#[cfg(test)]
mod tests;
