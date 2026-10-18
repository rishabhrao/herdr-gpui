//! The status sets drawn rather than loaded from assets: Superset's wave
//! grid, the braille cells, and Orca's marks. Every animation runs on the
//! app's synced clock under one id per kind, so all busy rows move in step,
//! and caps its repaints because a sidebar can show many at once. GPUI draws
//! the first frame under reduced motion, so each first frame is a complete,
//! readable mark.

mod grid;
mod orca;

use super::STATUS_DOT_UNKNOWN;
use crate::config::StatusIcons;
use gpui::{prelude::*, *};
use herdr_client::protocol::AgentStatus;
use std::time::Duration;

/// Superset's `AsciiSpinner` frames, 80ms each.
const BRAILLE: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const BRAILLE_FRAME_MS: u64 = 80;
/// A blocked cell empties dot by dot and fills again, after Zed's long
/// spinner. It starts full, so a held cell under reduced motion is solid.
const BREATHE: [&str; 14] = [
    "⣿", "⣷", "⣶", "⣦", "⣤", "⣄", "⣀", "⡀", "⣀", "⣄", "⣤", "⣦", "⣶", "⣷",
];
const BREATHE_FRAME_MS: u64 = 70;

/// The mark `set` draws for `status` in a `size` slot, in the status `color`.
pub(super) fn mark(set: StatusIcons, status: AgentStatus, size: f32, color: u32) -> AnyElement {
    match set {
        StatusIcons::Wave => grid::mark(status, size, color),
        StatusIcons::Orca => orca::mark(status, size, color),
        StatusIcons::Braille => braille_set(status, size, color),
        // The other sets load assets; only Dots' settled marks land here.
        _ => herdr_dot(status, size, color),
    }
}

/// The braille set: every status a braille cell, so the set reads as one
/// family. Working spins, blocked breathes, and the settled ones hold still.
fn braille_set(status: AgentStatus, size: f32, color: u32) -> AnyElement {
    let still = |selector: &'static str, glyph: &'static str| {
        braille_cell(selector, size, color)
            .child(glyph)
            .into_any_element()
    };
    match status {
        AgentStatus::Working => {
            braille("status-braille", &BRAILLE, BRAILLE_FRAME_MS, size, color).into_any_element()
        }
        AgentStatus::Blocked => braille(
            "status-braille-fill",
            &BREATHE,
            BREATHE_FRAME_MS,
            size,
            color,
        )
        .into_any_element(),
        AgentStatus::Done => still("status-braille-done", "⠿"),
        AgentStatus::Idle => still("status-braille-idle", "⠤"),
        AgentStatus::Unknown => still("status-braille-unknown", "⠄"),
    }
}

/// Herdr's dot at `size`: filled, hollow for idle, small for unknown.
fn herdr_dot(status: AgentStatus, size: f32, color: u32) -> AnyElement {
    match status {
        AgentStatus::Unknown => disc(STATUS_DOT_UNKNOWN, color).into_any_element(),
        AgentStatus::Idle => div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .border_1()
            .border_color(rgb(color))
            .into_any_element(),
        _ => disc(size, color).into_any_element(),
    }
}

/// Orca's dots are about half the icon slot.
fn dot_size(size: f32) -> f32 {
    (size * 0.5).round().max(4.)
}

fn disc(diameter: f32, color: u32) -> Div {
    div()
        .size(px(diameter))
        .flex_none()
        .rounded_full()
        .bg(rgb(color))
}

/// A slot holding one braille glyph, a line as tall as the slot.
fn braille_cell(selector: &'static str, size: f32, color: u32) -> Div {
    div()
        .debug_selector(move || selector.into())
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size))
        .line_height(px(size))
        .text_color(rgb(color))
}

/// A braille cell stepping through `frames`, one every `frame_ms`. The
/// selector names the animation too, so each kind keeps its own clock.
fn braille(
    selector: &'static str,
    frames: &'static [&'static str],
    frame_ms: u64,
    size: f32,
    color: u32,
) -> impl IntoElement {
    braille_cell(selector, size, color).with_animation(
        selector,
        Animation::new(Duration::from_millis(frame_ms * frames.len() as u64))
            .repeat_synced()
            .with_max_fps(1000. / frame_ms as f32),
        move |cell, delta| cell.child(frames[frame_index(frames.len(), delta)]),
    )
}

/// The frame a looping animation of `len` frames shows at `delta`, its share
/// of the loop: each frame holds for an equal slice, the last up to the end.
pub(super) fn frame_index(len: usize, delta: f32) -> usize {
    ((delta * len as f32) as usize).min(len.saturating_sub(1))
}

#[cfg(test)]
mod tests;
