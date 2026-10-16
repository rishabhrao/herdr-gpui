//! A swipe's momentum: after a quick swipe the content keeps moving and
//! slows down, as it does in any phone app, instead of stopping dead.
use crate::dom;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};

/// Velocities kept to measure a swipe, in milliseconds.
const WINDOW_MS: f64 = 100.;
/// Below this speed, in pixels per millisecond, a release just stops.
const MIN_SPEED: f64 = 0.15;
/// The share of speed kept each 16 ms frame.
const FRICTION: f64 = 0.95;

/// Recent finger positions with their times, to measure a swipe.
#[derive(Default)]
pub(crate) struct Tracker {
    samples: Vec<(f64, f64, f64)>,
}

impl Tracker {
    pub(crate) fn reset(&mut self) {
        self.samples.clear();
    }

    pub(crate) fn push(&mut self, time: f64, x: f64, y: f64) {
        self.samples.push((time, x, y));
        self.samples.retain(|(at, ..)| time - at <= WINDOW_MS);
    }

    /// The finger's speed at release, in pixels per millisecond.
    pub(crate) fn velocity(&self) -> Option<(f64, f64)> {
        let (first, last) = (self.samples.first()?, self.samples.last()?);
        let elapsed = last.0 - first.0;
        (elapsed >= 8.).then(|| ((last.1 - first.1) / elapsed, (last.2 - first.2) / elapsed))
    }
}

thread_local! {
    /// Bumped by each new fling and touch, which stop the one before.
    static GENERATION: RefCell<u32> = const { RefCell::new(0) };
}

/// Stops any fling, as a new touch does.
pub(crate) fn stop() {
    GENERATION.with(|generation| *generation.borrow_mut() += 1);
}

pub(crate) fn now() -> f64 {
    dom::window()
        .and_then(|window| window.performance())
        .map_or(0., |performance| performance.now())
}

/// Keeps moving at `velocity` after release, slowing each frame, and hands
/// `step` each frame's movement in pixels until it stops or returns false.
pub(crate) fn start(velocity: (f64, f64), step: impl FnMut(f64, f64) -> bool + 'static) {
    stop();
    let (vx, vy) = velocity;
    if vx.hypot(vy) < MIN_SPEED {
        return;
    }
    let generation = GENERATION.with(|generation| *generation.borrow());
    let state = Rc::new(RefCell::new((vx, vy, now(), step)));
    type Frame = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;
    let frame: Frame = Rc::new(RefCell::new(None));
    let again = frame.clone();
    *frame.borrow_mut() = Some(Closure::new(move || {
        let current = GENERATION.with(|current| *current.borrow());
        let done = {
            let mut state = state.borrow_mut();
            let (vx, vy, last, step) = &mut *state;
            let time = now();
            let elapsed = (time - *last).clamp(1., 64.);
            *last = time;
            let keep = FRICTION.powf(elapsed / 16.);
            *vx *= keep;
            *vy *= keep;
            current != generation
                || vx.hypot(*vy) < MIN_SPEED / 3.
                || !step(*vx * elapsed, *vy * elapsed)
        };
        if done {
            // A closure must not be dropped while it runs: a timer drops it,
            // which ends the cycle that kept it alive.
            let owner = again.clone();
            let release = Closure::once_into_js(move || {
                owner.borrow_mut().take();
            });
            if let Some(window) = dom::window() {
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    release.unchecked_ref(),
                    0,
                );
            }
            return;
        }
        if let (Some(window), Some(callback)) = (dom::window(), again.borrow().as_ref()) {
            let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
        }
    }));
    if let (Some(window), Some(callback)) = (dom::window(), frame.borrow().as_ref()) {
        let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
    }
}
