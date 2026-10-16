use super::*;

/// A sidebar showing one device follows the session's device; one showing
/// All Devices keeps showing all of them (the other tests).
#[gpui::test]
fn a_sidebar_showing_one_device_follows_the_chosen_session(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(move |window, cx| {
        let mut view = fixture_window(window, cx);
        view.endpoints[0].connection.target = development("default");
        view.sessions.entries = vec![session("default", SessionState::Running)];
        view.endpoints.push(crate::endpoint::Endpoint::new(
            "ssh:fixture".into(),
            "Build".into(),
            ConnectTarget::Ssh {
                target: "example.invalid".into(),
                session: "default".into(),
            },
            true,
        ));
        view.device_filter = Some(view.endpoints[0].id.clone());
        view
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    cx.run_until_parked();
    draw(cx);
    click(cx, "device-sessions");
    draw(cx);
    click(cx, "sessions-row-2");
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(view.selected_endpoint, 1);
        assert_eq!(view.device_filter.as_deref(), Some("ssh:fixture"));
    });
}
