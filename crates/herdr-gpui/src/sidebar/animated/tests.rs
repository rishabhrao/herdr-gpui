#![allow(clippy::unwrap_used)]
use super::*;
use core::prelude::v1::test;
use gpui::{Render, TestAppContext};

/// The wave's still frame lights the whole grid, so a held animation under
/// reduced motion still reads as working; the loop then empties every cell,
/// as Superset's does, before it starts again.
#[test]
fn the_wave_starts_full_and_empties_within_its_loop() {
    for diagonal in 0..5 {
        assert_eq!(
            grid::wave_opacity(grid::WAVE_FULL_FRAME, diagonal as f32),
            1.,
            "{diagonal}"
        );
    }
    let frames = 0..grid::WAVE_FRAMES as usize;
    for diagonal in 0..5 {
        let lit: Vec<f32> = frames
            .clone()
            .map(|frame| grid::wave_opacity(frame as f32, diagonal as f32))
            .collect();
        assert!(lit.iter().all(|value| (0. ..=1.).contains(value)));
        assert!(lit.contains(&0.), "{diagonal} never empties");
        assert!(lit.contains(&1.), "{diagonal} never fills");
    }
}

/// A blocked grid keeps its frame lit but for one cell, which visits every
/// frame cell once a loop; the center stays dark, so it never reads as done.
#[test]
fn the_blocked_gap_visits_every_frame_cell() {
    let mut visited = Vec::new();
    for gap in grid::FRAME {
        for index in 0..9 {
            let lit = grid::chase_opacity(gap, index);
            match index {
                4 => assert_eq!(lit, 0.),
                _ if index == gap => assert!(lit > 0. && lit < 1.),
                _ => assert_eq!(lit, 1.),
            }
        }
        assert!(!visited.contains(&gap));
        visited.push(gap);
    }
    assert_eq!(visited.len(), 8);
}

/// Orca's ring holds still between twelve evenly spaced steps a turn, and
/// starts upright so a held ring shows its gap at the top.
#[test]
fn the_orca_ring_steps_twelve_times_a_turn() {
    assert_eq!(orca::step(0.), 0.);
    let steps: Vec<f32> = (0..1200).map(|ms| orca::step(ms as f32 / 1200.)).collect();
    let mut distinct = steps.clone();
    distinct.dedup();
    assert_eq!(distinct.len(), orca::STEPS as usize);
    assert!(steps.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(steps.iter().all(|turn| (0. ..1.).contains(turn)));
}

/// The blocked cell starts full, so a held one under reduced motion reads as
/// a solid mark, and its loop returns to where it began.
#[test]
fn the_blocked_braille_cell_breathes_from_full() {
    assert_eq!(BREATHE[0], "⣿");
    assert_eq!(BREATHE[1], BREATHE[BREATHE.len() - 1]);
    let dots = |glyph: &str| (glyph.chars().next().unwrap() as u32 - 0x2800).count_ones();
    let counts: Vec<u32> = BREATHE.iter().map(|glyph| dots(glyph)).collect();
    let low = counts.iter().position(|&count| count == 1).unwrap();
    assert!(counts[..=low].windows(2).all(|pair| pair[0] > pair[1]));
    assert!(counts[low..].windows(2).all(|pair| pair[0] < pair[1]));
}

/// Across one loop a stepped animation shows every frame in order, each for
/// an equal slice, so the glyph or gap drawn changes as time passes.
#[test]
fn stepped_animations_walk_every_frame_in_order() {
    for len in [BRAILLE.len(), BREATHE.len(), grid::FRAME.len()] {
        let shown: Vec<usize> = (0..len * 10)
            .map(|step| frame_index(len, step as f32 / (len * 10) as f32))
            .collect();
        let mut order = shown.clone();
        order.dedup();
        assert_eq!(order, (0..len).collect::<Vec<_>>(), "{len} frames");
        assert_eq!(frame_index(len, 1.), len - 1);
    }
    // The drawn glyph differs between neighbouring slices of the loop.
    let at = |delta: f32| BRAILLE[frame_index(BRAILLE.len(), delta)];
    assert_ne!(at(0.), at(0.15));
    assert_ne!(
        BREATHE[frame_index(BREATHE.len(), 0.)],
        BREATHE[frame_index(BREATHE.len(), 0.5)]
    );
}

#[test]
fn spinner_frames_are_single_braille_cells() {
    for frame in BRAILLE.iter().chain(&BREATHE).chain(&["⠿", "⠤", "⠄"]) {
        let mut chars = frame.chars();
        let cell = chars.next().unwrap();
        assert!(chars.next().is_none(), "{frame}");
        assert!(('\u{2800}'..='\u{28ff}').contains(&cell), "{frame}");
    }
}

struct Mark {
    set: StatusIcons,
    status: AgentStatus,
}

impl Render for Mark {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(mark(self.set, self.status, 12., 0xf9e2af))
    }
}

/// The wave and braille sets draw their settled statuses in their own cells,
/// and Orca its question bubble and ringed check, each inside its slot.
#[gpui::test]
fn settled_marks_keep_their_family_inside_the_slot(cx: &mut TestAppContext) {
    for (set, status, selector) in [
        (StatusIcons::Wave, AgentStatus::Done, "status-grid-done"),
        (StatusIcons::Wave, AgentStatus::Idle, "status-grid-idle"),
        (
            StatusIcons::Wave,
            AgentStatus::Unknown,
            "status-grid-unknown",
        ),
        (
            StatusIcons::Braille,
            AgentStatus::Done,
            "status-braille-done",
        ),
        (
            StatusIcons::Braille,
            AgentStatus::Idle,
            "status-braille-idle",
        ),
        (
            StatusIcons::Braille,
            AgentStatus::Unknown,
            "status-braille-unknown",
        ),
        (
            StatusIcons::Orca,
            AgentStatus::Blocked,
            "status-orca-question",
        ),
        (StatusIcons::Orca, AgentStatus::Done, "status-orca-done"),
    ] {
        let (_, cx) = cx.add_window_view(|_, _| Mark { set, status });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{set:?} {status:?} draws no {selector}"));
        assert!(
            bounds.size.width <= px(12.) && bounds.size.height <= px(12.),
            "{set:?} {status:?} {bounds:?}"
        );
    }
}
