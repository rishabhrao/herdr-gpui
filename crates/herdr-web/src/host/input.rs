//! The page's input as GPUI's platform events.
use crate::wire::{Button, Delta, Input, Mods};
use gpui::{
    Capslock, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection, PinchEvent,
    Pixels, PlatformInput, Point, ScrollDelta, ScrollWheelEvent, TouchPhase, point, px,
};

pub(super) fn modifiers(mods: Mods) -> Modifiers {
    Modifiers {
        control: mods.control,
        alt: mods.alt,
        shift: mods.shift,
        platform: mods.platform,
        function: false,
    }
}

fn button(button: Button) -> MouseButton {
    match button {
        Button::Left => MouseButton::Left,
        Button::Middle => MouseButton::Middle,
        Button::Right => MouseButton::Right,
        Button::Back => MouseButton::Navigate(NavigationDirection::Back),
        Button::Forward => MouseButton::Navigate(NavigationDirection::Forward),
    }
}

fn position(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

fn keystroke(key: String, key_char: Option<String>, mods: Mods) -> Keystroke {
    Keystroke {
        modifiers: modifiers(mods),
        key,
        key_char,
    }
}

/// Whether a key press with `mods` types its character when nothing bound
/// it. Option composes text on macOS, so only Command and Control do not.
pub(super) fn inserts_text(mods: Mods) -> bool {
    !mods.platform && !mods.control
}

/// The event, and where it puts the pointer if it moves it.
pub(super) fn platform_input(input: Input) -> (PlatformInput, Option<Point<Pixels>>) {
    match input {
        Input::KeyDown {
            key,
            key_char,
            mods,
            held,
        } => (
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke: keystroke(key, key_char, mods),
                is_held: held,
                prefer_character_input: false,
            }),
            None,
        ),
        Input::KeyUp {
            key,
            key_char,
            mods,
        } => (
            PlatformInput::KeyUp(KeyUpEvent {
                keystroke: keystroke(key, key_char, mods),
            }),
            None,
        ),
        Input::Modifiers { mods, capslock } => (
            PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                modifiers: modifiers(mods),
                capslock: Capslock { on: capslock },
            }),
            None,
        ),
        Input::MouseDown {
            button: pressed,
            x,
            y,
            mods,
            clicks,
        } => (
            PlatformInput::MouseDown(MouseDownEvent {
                button: button(pressed),
                position: position(x, y),
                modifiers: modifiers(mods),
                click_count: usize::from(clicks.max(1)),
                first_mouse: false,
            }),
            Some(position(x, y)),
        ),
        Input::MouseUp {
            button: released,
            x,
            y,
            mods,
            clicks,
        } => (
            PlatformInput::MouseUp(MouseUpEvent {
                button: button(released),
                position: position(x, y),
                modifiers: modifiers(mods),
                click_count: usize::from(clicks.max(1)),
            }),
            Some(position(x, y)),
        ),
        Input::MouseMove {
            x,
            y,
            pressed,
            mods,
        } => (
            PlatformInput::MouseMove(MouseMoveEvent {
                position: position(x, y),
                pressed_button: pressed.map(button),
                modifiers: modifiers(mods),
            }),
            Some(position(x, y)),
        ),
        Input::MouseExit {
            x,
            y,
            pressed,
            mods,
        } => (
            PlatformInput::MouseExited(MouseExitEvent {
                position: position(x, y),
                pressed_button: pressed.map(button),
                modifiers: modifiers(mods),
            }),
            None,
        ),
        Input::Scroll { x, y, delta, mods } => (
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: position(x, y),
                delta: match delta {
                    Delta::Pixels(dx, dy) => ScrollDelta::Pixels(point(px(dx), px(dy))),
                    Delta::Lines(dx, dy) => ScrollDelta::Lines(point(dx, dy)),
                },
                modifiers: modifiers(mods),
                touch_phase: TouchPhase::Moved,
            }),
            Some(position(x, y)),
        ),
        Input::Pinch { x, y, delta, mods } => (
            PlatformInput::Pinch(PinchEvent {
                position: position(x, y),
                delta,
                modifiers: modifiers(mods),
                phase: TouchPhase::Moved,
            }),
            Some(position(x, y)),
        ),
    }
}
