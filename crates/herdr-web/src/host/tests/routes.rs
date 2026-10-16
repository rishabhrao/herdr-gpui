use super::*;

const DEVICE_COOKIE: &str = "herdr_device_4321";
use crate::host::{
    gate::Door,
    routes::{self, device_cookie_name},
    server,
};

fn json(response: &crate::host::http::Response) -> serde_json::Value {
    serde_json::from_slice(&response.body).unwrap()
}

fn cookie_header(name: &str, value: &str) -> (&'static str, String) {
    ("Cookie", format!("{name}={value}"))
}

#[test]
fn the_token_address_pairs_this_mac_and_leaves_the_address_bar() {
    let fixture = fixture("local");
    let hub = &fixture.hub;
    let opened = routes::get(
        hub,
        &with("GET", "/", &format!("token={}", fixture.token), &local(&[])),
        Door::Local,
    );
    assert_eq!(opened.status, 302);
    let token = set_cookie(&opened, DEVICE_COOKIE).unwrap();
    assert!(
        !opened
            .headers
            .iter()
            .any(|(_, value)| value.contains("Secure"))
    );
    let status = routes::get(
        hub,
        &with(
            "GET",
            "/auth/status",
            "",
            &local(&[cookie_header(DEVICE_COOKIE, &token)]),
        ),
        Door::Local,
    );
    assert_eq!(json(&status)["paired"], true);
    assert_eq!(json(&status)["label"], "This Mac");

    let wrong = routes::get(hub, &with("GET", "/", "token=00", &local(&[])), Door::Local);
    assert_eq!(wrong.status, 403);
    // Through the tunnel the token address pairs nothing.
    let remote = routes::get(
        hub,
        &with(
            "GET",
            "/",
            &format!("token={}", fixture.token),
            &public(&[]),
        ),
        Door::Public,
    );
    assert_eq!(remote.status, 200);
    assert!(set_cookie(&remote, DEVICE_COOKIE).is_none());
}

#[test]
fn an_unpaired_browser_gets_only_the_page_and_the_pairing_screen() {
    let fixture = fixture("unpaired");
    let hub = &fixture.hub;
    let page = routes::get(hub, &with("GET", "/", "", &public(&[])), Door::Public);
    assert_eq!(page.status, 200);
    assert!(
        page.headers
            .iter()
            .any(|(name, value)| *name == "Content-Security-Policy"
                && value.contains("wss://herdr.example.com"))
    );
    assert_eq!(
        json(&routes::get(
            hub,
            &with("GET", "/auth/status", "", &public(&[])),
            Door::Public
        ))["paired"],
        false
    );
    assert_eq!(
        routes::get(
            hub,
            &with("GET", "/auth/devices", "", &public(&[])),
            Door::Public
        )
        .status,
        401
    );
    assert_eq!(
        routes::get(
            hub,
            &with("GET", "/page/1/abc/index.html", "", &public(&[])),
            Door::Public
        )
        .status,
        401
    );
    let forged = cookie_header(DEVICE_COOKIE, &random_hex(32).unwrap());
    assert_eq!(
        json(&routes::get(
            hub,
            &with("GET", "/auth/status", "", &public(&[forged])),
            Door::Public
        ))["paired"],
        false
    );
}

#[test]
fn a_phone_pairs_once_someone_approves_its_code() {
    let fixture = fixture("pair");
    let hub = &fixture.hub;
    let asked = routes::post(
        hub,
        &with("POST", "/pair/request", "", &public(&[])),
        Door::Public,
        br#"{"label":"iPhone Safari"}"#,
    );
    assert_eq!(asked.status, 200);
    let reply = json(&asked);
    let secret = set_cookie(&asked, "herdr_pair_4321").unwrap();
    assert!(
        asked
            .headers
            .iter()
            .any(|(_, value)| value.starts_with("herdr_pair_4321=")
                && value.contains("Secure")
                && value.contains("HttpOnly"))
    );

    // The app hears of it, with the code the phone shows.
    let request = fixture.approvals.try_recv().unwrap();
    assert_eq!(request.code, reply["code"]);
    assert_eq!(request.label, "iPhone Safari");

    let wait = |cookie: &str| {
        routes::get(
            hub,
            &with(
                "GET",
                "/pair/wait",
                &format!("id={}", reply["id"]),
                &public(&[cookie_header("herdr_pair_4321", cookie)]),
            ),
            Door::Public,
        )
    };
    assert_eq!(json(&wait(&secret))["state"], "waiting");
    assert!(hub.devices.answer(request.id, true).unwrap());
    // Another browser cannot collect the token.
    assert_eq!(json(&wait("0123"))["state"], "gone");
    let approved = wait(&secret);
    assert_eq!(json(&approved)["state"], "approved");
    let token = set_cookie(&approved, DEVICE_COOKIE).unwrap();
    let status = routes::get(
        hub,
        &with(
            "GET",
            "/auth/status",
            "",
            &public(&[cookie_header(DEVICE_COOKIE, &token)]),
        ),
        Door::Public,
    );
    assert_eq!(json(&status)["paired"], true);
}

#[test]
fn another_site_cannot_ask_or_forget_for_the_user() {
    let fixture = fixture("csrf");
    let hub = &fixture.hub;
    let mut headers = public(&[]);
    headers.retain(|(name, _)| *name != "Origin");
    headers.push(("Origin", "https://evil.example".into()));
    let asked = routes::post(
        hub,
        &with("POST", "/pair/request", "", &headers),
        Door::Public,
        br#"{"label":"x"}"#,
    );
    assert_eq!(asked.status, 403);
    assert!(fixture.approvals.try_recv().is_err());
}

#[test]
fn forgetting_this_device_signs_it_out() {
    let fixture = fixture("signout");
    let hub = &fixture.hub;
    let (device, token) = hub.devices.add("Phone", true).unwrap();
    let cookie = cookie_header(DEVICE_COOKIE, &token);
    let listed = routes::get(
        hub,
        &with(
            "GET",
            "/auth/devices",
            "",
            &public(std::slice::from_ref(&cookie)),
        ),
        Door::Public,
    );
    assert_eq!(json(&listed)["devices"][0]["current"], true);
    let body = format!(r#"{{"id":"{}"}}"#, device.id);
    let forgot = routes::post(
        hub,
        &with(
            "POST",
            "/auth/forget",
            "",
            &public(std::slice::from_ref(&cookie)),
        ),
        Door::Public,
        body.as_bytes(),
    );
    assert_eq!(json(&forgot)["ok"], true);
    assert_eq!(set_cookie(&forgot, DEVICE_COOKIE).as_deref(), Some(""));
    assert!(routes::device(hub, &with("GET", "/", "", &public(&[cookie])), Door::Public).is_none());
}

#[test]
fn assets_are_checked_rather_than_downloaded_again() {
    let fixture = fixture("etag");
    let hub = &fixture.hub;
    let first = routes::get(
        hub,
        &with("GET", "/herdr_web_client_bg.wasm", "", &public(&[])),
        Door::Public,
    );
    assert_eq!(
        (first.status, first.content_type),
        (200, "application/wasm")
    );
    assert_eq!(first.cache, "private, no-cache");
    let etag = first
        .headers
        .iter()
        .find(|(name, _)| *name == "ETag")
        .unwrap()
        .1
        .clone();
    let again = routes::get(
        hub,
        &with(
            "GET",
            "/herdr_web_client_bg.wasm",
            "",
            &public(&[("If-None-Match", etag)]),
        ),
        Door::Public,
    );
    assert_eq!(again.status, 304);
    assert!(again.body.is_empty());
    for path in [
        "/manifest.webmanifest",
        "/sw.js",
        "/icon-192.png",
        "/chrome.js",
    ] {
        assert_eq!(
            routes::get(hub, &with("GET", path, "", &public(&[])), Door::Public).status,
            200,
            "{path}"
        );
    }
    assert_eq!(
        routes::get(
            hub,
            &with("GET", "/secret.txt", "", &public(&[])),
            Door::Public
        )
        .status,
        404
    );
}

#[test]
fn the_front_door_refuses_unknown_hosts() {
    let fixture = fixture("admit");
    let hub = &fixture.hub;
    assert_eq!(
        server::admit(hub, &with("GET", "/", "", &public(&[]))).ok(),
        Some(Door::Public)
    );
    assert_eq!(
        server::admit(hub, &with("GET", "/", "", &local(&[]))).ok(),
        Some(Door::Local)
    );
    let rebound = request("GET", "/", "", &[("Host", "evil.example")]);
    assert_eq!(
        server::admit(hub, &rebound)
            .err()
            .map(|response| response.status),
        Some(403)
    );
}

#[test]
fn each_port_has_its_own_cookie_and_the_old_name_still_works() {
    let fixture = fixture("port-cookie");
    let hub = &fixture.hub;
    assert_eq!(device_cookie_name(PORT), DEVICE_COOKIE);
    let (_, token) = hub.devices.add("Old browser", false).unwrap();
    let legacy = ("Cookie", format!("herdr_device={token}"));
    assert!(routes::device(hub, &with("GET", "/", "", &public(&[legacy])), Door::Public).is_some());
    // Another server's cookie under the old name does not hide this one's.
    let both = (
        "Cookie",
        format!("herdr_device=0000; {DEVICE_COOKIE}={token}"),
    );
    assert!(routes::device(hub, &with("GET", "/", "", &public(&[both])), Door::Public).is_some());
}

#[test]
fn a_browser_on_this_mac_needs_no_pairing() {
    let fixture = fixture("this-mac");
    let hub = &fixture.hub;
    let device = routes::device(hub, &with("GET", "/", "", &local(&[])), Door::Local);
    assert_eq!(
        device.map(|device| device.label),
        Some("This Mac".to_owned())
    );
    // Through the tunnel, the same request is a stranger.
    assert!(routes::device(hub, &with("GET", "/", "", &public(&[])), Door::Public).is_none());
}
