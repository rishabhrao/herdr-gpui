//! **Build sandbox images** in a workspace's menu: build the repository's
//! image ahead of a dispatch, for every provider a dispatch could make a new
//! machine from that holds images, so the first new machine starts at once.

use crate::{HerdrWindow, cloud::CloudProvider, window::Flash};
use gpui::{Context, Window};

impl HerdrWindow {
    /// The providers this workspace's repository can have images built for.
    pub(crate) fn sandbox_image_providers(&self) -> Vec<CloudProvider> {
        self.dispatch_sandboxes()
            .into_iter()
            .filter(|&provider| crate::cloud::image_support(provider).is_some())
            .collect()
    }

    pub(super) fn build_workspace_sandbox_images(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((origin, path)) = self.menu.target.as_ref().and_then(|target| {
            let tree = target.worktree.as_ref()?;
            let origin = self.dispatch_origin(&target.id, &tree.key, &tree.label)?;
            let path = crate::cloud::image::checkout_of(&origin.repo_key)?.to_owned();
            Some((origin, path))
        }) else {
            self.show_flash(
                Flash::warning("This repository has no checkout to build an image from"),
                cx,
            );
            return;
        };
        self.dismiss_menu(window, cx);
        let mut started = Vec::new();
        for provider in self.sandbox_image_providers() {
            match self.build_sandbox_image(
                provider,
                &origin.place.endpoint_id,
                path.clone(),
                origin.repo_label.clone(),
                cx,
            ) {
                Ok(()) => started.push(crate::cloud::name(provider)),
                Err(error) => {
                    self.show_flash(Flash::warning(format!("Not built: {error}")), cx);
                    return;
                }
            }
        }
        self.show_flash(
            Flash::success(format!(
                "Building {}'s images for {}; Settings > Cloud devices shows their progress",
                origin.repo_label,
                started.join(" and ")
            )),
            cx,
        );
    }
}
