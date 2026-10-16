//! The Settings pages and the groups the section list shows them under.

/// One Settings page. `ALL` is the section list's order, group by group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Section {
    #[default]
    Theme,
    Sidebar,
    Fonts,
    StatusBar,
    General,
    Tabs,
    Code,
    Notifications,
    Integrations,
    Plugins,
    WebAccess,
    #[cfg(feature = "cloud")]
    CloudDevices,
    Configuration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Group {
    Appearance,
    Behaviour,
    Connect,
    Advanced,
}

impl Group {
    pub(super) const ALL: [Self; 4] = [
        Self::Appearance,
        Self::Behaviour,
        Self::Connect,
        Self::Advanced,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Behaviour => "Behaviour",
            Self::Connect => "Connect",
            Self::Advanced => "Advanced",
        }
    }
}

impl Section {
    /// The section list's order: each group's pages together, groups in
    /// [`Group::ALL`] order. Users see this order, so it is not sorted.
    pub(super) const ALL: &[Self] = &[
        Self::Theme,
        Self::Sidebar,
        Self::Fonts,
        Self::StatusBar,
        Self::General,
        Self::Tabs,
        Self::Code,
        Self::Notifications,
        Self::Integrations,
        Self::Plugins,
        Self::WebAccess,
        #[cfg(feature = "cloud")]
        Self::CloudDevices,
        Self::Configuration,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Sidebar => "Sidebar",
            Self::Fonts => "Fonts",
            Self::StatusBar => "Status bar",
            Self::General => "General",
            Self::Tabs => "Tabs & panes",
            Self::Code => "Code",
            Self::Notifications => "Notifications",
            Self::Integrations => "Integrations",
            Self::Plugins => "Plugins",
            Self::WebAccess => "Web access",
            #[cfg(feature = "cloud")]
            Self::CloudDevices => "Cloud devices",
            Self::Configuration => "Configuration",
        }
    }

    pub(super) fn lead(self) -> Option<&'static str> {
        match self {
            Self::Theme => Some("Hover a theme to try it in the app; click to keep it."),
            Self::Sidebar => Some("Changes apply to every open window immediately."),
            Self::Fonts => Some("Each face can have its own family and size."),
            Self::StatusBar => Some("Which items the bottom bar shows, and how compactly."),
            Self::Code => Some(
                "Review code and diffs beside your terminals through a Visual Studio Code server.",
            ),
            Self::Integrations => Some("Agents Herdr can launch and track."),
            Self::Plugins => Some("Show what your plugins report in the sidebar."),
            Self::WebAccess => Some("Use this app in a browser, on this Mac or another device."),
            #[cfg(feature = "cloud")]
            Self::CloudDevices => {
                Some("Create machines from your cloud accounts and use them as devices.")
            }
            Self::Configuration => {
                Some("Files behind these settings. Saved edits reload automatically.")
            }
            Self::General | Self::Tabs | Self::Notifications => None,
        }
    }

    pub(super) fn group(self) -> Group {
        match self {
            Self::Theme | Self::Sidebar | Self::Fonts | Self::StatusBar => Group::Appearance,
            Self::General | Self::Tabs | Self::Code | Self::Notifications => Group::Behaviour,
            Self::Integrations | Self::Plugins | Self::WebAccess => Group::Connect,
            #[cfg(feature = "cloud")]
            Self::CloudDevices => Group::Connect,
            Self::Configuration => Group::Advanced,
        }
    }

    pub(super) fn icon(self) -> &'static str {
        match self {
            Self::Theme => "icons/theme.svg",
            Self::Sidebar => "icons/panel-left.svg",
            Self::Fonts => "icons/pencil.svg",
            Self::StatusBar => "icons/status-bar.svg",
            Self::General => "icons/settings.svg",
            Self::Tabs => "icons/sessions.svg",
            Self::Code => "icons/vscode.svg",
            Self::Notifications => "icons/bell.svg",
            Self::Integrations => "icons/agent-generic.svg",
            Self::Plugins => "icons/plug.svg",
            Self::WebAccess => "icons/globe.svg",
            #[cfg(feature = "cloud")]
            Self::CloudDevices => "icons/globe.svg",
            Self::Configuration => "icons/file.svg",
        }
    }

    pub(super) fn selector(self) -> String {
        format!(
            "settings-section-{}",
            self.label()
                .to_lowercase()
                .replace(' ', "-")
                .replace('&', "and")
        )
    }

    /// The section's index among the scrolled section list's children, which
    /// are each group's label followed by its pages.
    pub(super) fn nav_item(self) -> usize {
        let mut item = 0;
        for group in Group::ALL {
            item += 1;
            for section in Self::ALL.iter().filter(|section| section.group() == group) {
                if *section == self {
                    return item;
                }
                item += 1;
            }
        }
        item
    }
}
