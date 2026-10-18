//! Agent status indicators: which of this client's sets draws status, and
//! the edge rail. The Indicators page renders the choice; this keeps how a
//! pick is saved.
use super::*;
use crate::config::{StatusIcons, preferences::Preference};

/// The name the Indicators page gives a set.
pub(in crate::settings_window) fn set_label(icons: StatusIcons) -> &'static str {
    match icons {
        StatusIcons::Dots => "Dots",
        StatusIcons::Solid => "Solid",
        StatusIcons::Outline => "Outline",
        StatusIcons::Tiles => "Tiles",
        StatusIcons::Wave => "Wave",
        StatusIcons::Braille => "Braille",
        StatusIcons::Orca => "Orca",
    }
}

/// One line on what sets a set apart, under its preview.
pub(in crate::settings_window) fn set_description(icons: StatusIcons) -> &'static str {
    match icons {
        StatusIcons::Dots => "Small dots; a ring with a turning arc while working.",
        StatusIcons::Solid => "Filled discs with a cut-out mark; a crescent when idle.",
        StatusIcons::Outline => "The same marks in outline, with an open arc spinning.",
        StatusIcons::Tiles => "Each mark on a tile tinted with its status color.",
        StatusIcons::Wave => "Superset's wave grid; every status drawn in its cells.",
        StatusIcons::Braille => "Braille cells; blocked fills and empties dot by dot.",
        StatusIcons::Orca => "Orca's stepping ring, question bubble, and ringed check.",
    }
}

impl SettingsWindow {
    /// Picks the set status is drawn with. The set already in use saves
    /// nothing. A retired `rail` value turned the rail on with no `edge_rail`
    /// key, so an on rail is written with the set to stay on once that value
    /// is replaced.
    pub(in crate::settings_window) fn choose_status_icons(
        &mut self,
        icons: StatusIcons,
        cx: &mut Context<Self>,
    ) {
        if self.config.status_icons == icons {
            return;
        }
        let edits: Vec<Preference> = self
            .config
            .edge_rail
            .then_some(Preference::EdgeRail(true))
            .into_iter()
            .chain([Preference::StatusIcons(icons)])
            .collect();
        #[cfg(test)]
        if let Some(io) = self.controls.preference_io.clone() {
            let load = io.load.clone();
            self.save_with(
                move || edits.into_iter().try_for_each(|edit| (io.write)(edit)),
                move || load(),
                false,
                cx,
            );
            return;
        }
        self.save_native(
            move || edits.into_iter().try_for_each(Config::save_preference),
            cx,
        );
    }
}
