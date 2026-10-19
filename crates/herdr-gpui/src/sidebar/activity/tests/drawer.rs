//! On a phone the sidebar is a drawer over the window. Choosing a place in
//! it shows that place, not the section the drawer was opened over.
use super::{
    sections::{local, show},
    *,
};
use crate::navigation::NavigationTarget;

fn choose_workspace(view: &gpui::Entity<HerdrWindow>, cx: &mut gpui::VisualTestContext) {
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.navigate(NavigationTarget::Workspace("w2"), cx);
        })
    });
}

fn state(
    view: &gpui::Entity<HerdrWindow>,
    cx: &mut gpui::VisualTestContext,
) -> (bool, ActivityPanel) {
    cx.update(|_, cx| {
        let view = view.read(cx);
        (view.sidebar_drawer, view.activity.panel)
    })
}

#[gpui::test]
fn a_workspace_chosen_in_the_drawer_leaves_the_section(cx: &mut gpui::TestAppContext) {
    let (view, cx) = local(cx);
    show(&view, cx, ActivityPanel::Overview);
    cx.update(|_, cx| view.update(cx, |view, _| view.sidebar_drawer = true));
    choose_workspace(&view, cx);
    assert_eq!(state(&view, cx), (false, ActivityPanel::Workspaces));
}

#[gpui::test]
fn a_workspace_chosen_elsewhere_keeps_the_section(cx: &mut gpui::TestAppContext) {
    let (view, cx) = local(cx);
    show(&view, cx, ActivityPanel::Overview);
    choose_workspace(&view, cx);
    assert_eq!(state(&view, cx), (false, ActivityPanel::Overview));
}
