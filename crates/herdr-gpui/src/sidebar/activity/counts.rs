//! The bar's count bubbles: what needs the user, as the Overview counted it
//! on the last tick, and from the orchestrator views the window holds, open
//! pull requests and open issues. An item two views list, such as one
//! repository open in two workspaces, counts once.

use super::ActivityPanel;
use crate::{HerdrWindow, orchestrator::Item};
use gpui::App;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Counts {
    needs: usize,
    pull_requests: usize,
    issues: usize,
}

impl Counts {
    /// One view's items added to the counts, skipping what `seen` holds.
    pub(super) fn add(&mut self, seen: &mut HashSet<String>, items: &[Item]) {
        for item in items {
            if !open(item) || !seen.insert(item.key.canonical()) {
                continue;
            }
            if item.pull_request.is_some() {
                self.pull_requests += 1;
            } else {
                self.issues += 1;
            }
        }
    }

    /// The bar's count for `panel`, if it shows one.
    pub(super) fn of(self, panel: ActivityPanel) -> Option<usize> {
        match panel {
            ActivityPanel::Overview => Some(self.needs),
            ActivityPanel::Workspaces | ActivityPanel::Usage => None,
            ActivityPanel::PullRequests => Some(self.pull_requests),
            ActivityPanel::Issues => Some(self.issues),
        }
    }
}

/// Whether the provider still counts the item as open. Beads' own word for
/// finished work counts as closed too.
fn open(item: &Item) -> bool {
    !matches!(item.state.as_str(), "closed" | "merged" | "done")
}

impl HerdrWindow {
    pub(super) fn activity_counts(&self, cx: &App) -> Counts {
        let mut counts = Counts {
            needs: self.overview_needing(),
            ..Counts::default()
        };
        let mut seen = HashSet::new();
        for (_, view) in self.orchestrator_views() {
            for (_, items, _) in view.read(cx).listed() {
                counts.add(&mut seen, items);
            }
        }
        counts
    }
}
