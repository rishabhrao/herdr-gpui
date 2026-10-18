//! Small pieces the chrome uses to tell screen readers what GPUI's own
//! `aria_*` setters cannot, through AccessKit.

use gpui::{App, Context, StatefulInteractiveElement, WeakEntity, Window, accesskit::ActionData};

/// Marks an element unavailable, so a screen reader says so and offers none
/// of its actions. GPUI has no setter for it, so the node is changed directly
/// once built.
pub(crate) fn disabled<E: StatefulInteractiveElement>(element: E, disabled: bool) -> E {
    if !disabled {
        return element;
    }
    element.a11y_synthetic_children(|builder| builder.parent_node().set_disabled())
}

/// A screen reader's action listener that runs `f` on a view, with the window.
pub(crate) fn update<T: 'static>(
    view: WeakEntity<T>,
    f: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
) -> impl FnMut(Option<&ActionData>, &mut Window, &mut App) + 'static {
    move |_, window, cx| {
        let _ = view.update(cx, |this, cx| f(this, window, cx));
    }
}
