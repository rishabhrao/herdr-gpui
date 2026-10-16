//! Shared row and control builders for the Settings pages. Pure rendering:
//! they take the theme and return elements; state and saves stay on the view.
use crate::config::{Theme, corners};
use gpui::{prelude::*, *};
use std::rc::Rc;

mod number;
mod popover;
mod swatches;
pub(in crate::settings_window) use number::{
    Number, NumberChange, NumberEdit, number, parse_number,
};
pub(in crate::settings_window) use popover::{Dropdown, dropdown};
pub(in crate::settings_window) use swatches::{HostColorChange, parse_hex_input, swatch_row};

pub(in crate::settings_window) const ROW_HEIGHT: f32 = 38.;

/// Where a setting is stored: shared with every machine or local to this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::settings_window) enum Source {
    Shared,
    Local,
}

pub(in crate::settings_window) fn tag(theme: &Theme, source: Source) -> Div {
    let (label, color) = match source {
        Source::Shared => ("Shared", theme.ink(0x8fb8d9)),
        Source::Local => ("Local", theme.ink(0xc9b26f)),
    };
    div()
        .text_size(px(9.))
        .line_height(px(13.))
        .px(px(5.))
        .rounded(px(corners::SMALL))
        .border_1()
        .border_color(rgb(theme.active))
        .text_color(rgb(color))
        .font_weight(FontWeight::MEDIUM)
        .child(label)
}

pub(in crate::settings_window) fn section(
    theme: &Theme,
    title: &'static str,
    tag_source: Option<Source>,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .mt(px(22.))
        .pb(px(6.))
        .border_b_1()
        .border_color(rgb(theme.active))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme.subtext()))
        .child(title.to_uppercase())
        .children(tag_source.map(|source| tag(theme, source)))
}

pub(in crate::settings_window) struct Row {
    theme: Theme,
    id: SharedString,
    title: SharedString,
    description: Option<SharedString>,
    tag: Option<Source>,
    disabled: Option<SharedString>,
    controls: Vec<AnyElement>,
}

pub(in crate::settings_window) fn row(
    theme: &Theme,
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
) -> Row {
    Row {
        theme: theme.clone(),
        id: id.into(),
        title: title.into(),
        description: None,
        tag: None,
        disabled: None,
        controls: Vec::new(),
    }
}

impl Row {
    pub(in crate::settings_window) fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }
    pub(in crate::settings_window) fn tag(mut self, source: Source) -> Self {
        self.tag = Some(source);
        self
    }
    /// Dims the row's text and shows `reason` in place of the description.
    /// Its controls dim themselves when the caller builds them disabled.
    pub(in crate::settings_window) fn disabled(mut self, reason: impl Into<SharedString>) -> Self {
        self.disabled = Some(reason.into());
        self
    }
    pub(in crate::settings_window) fn control(mut self, control: impl IntoElement) -> Self {
        self.controls.push(control.into_any_element());
        self
    }
}

impl IntoElement for Row {
    type Element = Div;
    fn into_element(self) -> Div {
        let theme = &self.theme;
        let id = self.id.clone();
        let title_id = format!("{id}-title");
        let desc_id = format!("{id}-description");
        let tag_id = format!("{id}-tag");
        let dimmed = self.disabled.is_some();
        let tracked = id.clone();
        let reason_id = format!("{id}-reason");
        // A disabled row shows its reason instead of the description.
        let (description, desc_id) = match self.disabled {
            Some(reason) => (Some(reason), reason_id),
            None => (self.description, desc_id),
        };
        div()
            .debug_selector(move || id.to_string())
            .relative()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(16.))
            .gap_y(px(6.))
            .min_h(px(ROW_HEIGHT))
            .py(px(6.))
            .border_b_1()
            .border_color(rgb(theme.active))
            .child(super::search::track_bounds(tracked))
            .child(
                // Only the text dims: a control a disabled row cannot use is
                // built disabled and dims itself, and GPUI multiplies nested
                // opacity, so dimming the whole row would show it at 25 %.
                // Narrower than this, the controls go under the text.
                div()
                    .flex_1()
                    .min_w(px(160.))
                    .when(dimmed, |text| text.opacity(0.5))
                    .child(
                        div()
                            .debug_selector(move || title_id.clone())
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.title)
                            .children(self.tag.map(|source| {
                                tag(theme, source).debug_selector(move || tag_id.clone())
                            })),
                    )
                    .children(description.map(|text| {
                        div()
                            .debug_selector(move || desc_id.clone())
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .text_color(rgb(theme.subtext()))
                            .child(text)
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .flex_none()
                    .children(self.controls),
            )
    }
}

/// The switch track alone; the row carries the label.
pub(in crate::settings_window) fn toggle(
    theme: &Theme,
    id: impl Into<SharedString>,
    checked: bool,
    enabled: bool,
) -> Stateful<Div> {
    let id = id.into();
    crate::toggles::switch(theme, 20., checked)
        .id(ElementId::Name(id.clone()))
        .debug_selector(move || id.to_string())
        .when(enabled, |track| track.cursor_pointer())
        .when(!enabled, |track| track.opacity(0.5))
}

/// A bordered text button; the caller adds `.on_click` when it is enabled.
pub(in crate::settings_window) fn button(
    theme: &Theme,
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    enabled: bool,
) -> Stateful<Div> {
    let id = id.into();
    div()
        .id(ElementId::Name(id.clone()))
        .debug_selector(move || id.to_string())
        .flex_none()
        .whitespace_nowrap()
        .px(px(10.))
        .py(px(3.))
        .rounded(px(corners::CONTROL))
        .border_1()
        .border_color(rgb(theme.active))
        .bg(rgb(theme.surface))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(rgb(theme.active)))
        })
        .when(!enabled, |button| button.opacity(0.5))
        .child(label.into())
}

pub(in crate::settings_window) fn segmented<T: Copy + PartialEq + 'static>(
    theme: &Theme,
    id: &'static str,
    options: &[(T, &'static str)],
    selected: Option<T>,
    enabled: bool,
    on_pick: impl Fn(T, &mut Window, &mut App) + 'static,
) -> Div {
    let options = options
        .iter()
        .map(|(value, label)| (*value, (*label).into_any_element()))
        .collect();
    segmented_elements(theme, id, options, selected, enabled, |_| true, on_pick)
}

/// Like [`segmented`], with an element per segment (e.g. coloured status marks).
/// `live` says which segments respond while the strip is enabled; the others
/// are dimmed and ignore clicks.
pub(in crate::settings_window) fn segmented_elements<T: Copy + PartialEq + 'static>(
    theme: &Theme,
    id: &'static str,
    options: Vec<(T, AnyElement)>,
    selected: Option<T>,
    enabled: bool,
    live: impl Fn(T) -> bool,
    on_pick: impl Fn(T, &mut Window, &mut App) + 'static,
) -> Div {
    let on_pick = Rc::new(on_pick);
    div()
        .flex()
        .flex_none()
        .p(px(2.))
        .rounded(px(corners::CONTROL))
        .border_1()
        .border_color(rgb(theme.active))
        .bg(rgb(theme.surface))
        .when(!enabled, |strip| strip.opacity(0.5))
        .children(
            options
                .into_iter()
                .enumerate()
                .map(|(index, (value, label))| {
                    let on = selected == Some(value);
                    let segment_live = enabled && live(value);
                    let on_pick = on_pick.clone();
                    div()
                        .id((id, index))
                        .debug_selector(move || format!("{id}-{index}"))
                        .px(px(10.))
                        .py(px(3.))
                        .rounded(px(corners::SMALL))
                        .whitespace_nowrap()
                        .text_color(rgb(if on {
                            theme.foreground
                        } else {
                            theme.subtext()
                        }))
                        .when(on, |seg| seg.bg(rgb(theme.active)))
                        .when(enabled && !segment_live, |seg| seg.opacity(0.5))
                        .when(segment_live, |seg| seg.cursor_pointer())
                        .when(segment_live && !on, move |seg| {
                            seg.on_click(move |_, window, cx| on_pick(value, window, cx))
                        })
                        .child(label)
                }),
        )
}

#[cfg(test)]
mod tests;
