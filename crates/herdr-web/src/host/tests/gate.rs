use super::*;
use crate::host::gate::{Door, Doors, host_name};

fn doors() -> Doors {
    Doors::new(
        PORT,
        &[format!("https://{PUBLIC}/"), "HERDR.example.com".into()],
    )
}

#[test]
fn public_hosts_are_normalized_once() {
    assert_eq!(doors().public_hosts(), [PUBLIC]);
    assert_eq!(
        host_name("https://Example.com:443/path"),
        Some("example.com".into())
    );
    assert_eq!(host_name("  "), None);
    assert_eq!(host_name("evil.com/<script>"), Some("evil.com".into()));
    assert_eq!(host_name("bad host"), None);
}

#[test]
fn each_request_comes_through_a_known_door_or_none() {
    let doors = doors();
    let door = |headers: &[(&str, &str)]| doors.door(&request("GET", "/", "", headers));
    assert_eq!(door(&[("Host", "localhost:4321")]), Some(Door::Local));
    assert_eq!(door(&[("Host", "127.0.0.1:4321")]), Some(Door::Local));
    assert_eq!(door(&[("Host", PUBLIC)]), Some(Door::Public));
    assert_eq!(
        door(&[("Host", "herdr.example.com:443")]),
        Some(Door::Public)
    );
    // DNS rebinding: a name nobody configured.
    assert_eq!(door(&[("Host", "evil.example:4321")]), None);
    assert_eq!(door(&[("Host", "localhost:9999")]), None);
    assert_eq!(door(&[]), None);
    // A tunnel that rewrites Host to loopback still forwards for a stranger.
    assert_eq!(
        door(&[
            ("Host", "localhost:4321"),
            ("Cf-Connecting-Ip", "203.0.113.9")
        ]),
        Some(Door::Public)
    );
    assert_eq!(
        door(&[
            ("Host", "localhost:4321"),
            ("X-Forwarded-For", "203.0.113.9")
        ]),
        Some(Door::Public)
    );
}

#[test]
fn only_the_doors_own_page_origin_is_accepted() {
    let doors = doors();
    let origin =
        |value: &str, door| doors.origin_ok(&request("GET", "/ws", "", &[("Origin", value)]), door);
    assert!(origin("http://localhost:4321", Door::Local));
    assert!(origin("http://127.0.0.1:4321", Door::Local));
    assert!(!origin("https://herdr.example.com", Door::Local));
    assert!(origin("https://herdr.example.com", Door::Public));
    // Plain HTTP to a public host is not this app's page.
    assert!(!origin("http://herdr.example.com", Door::Public));
    assert!(!origin("https://evil.example", Door::Public));
    assert!(!doors.origin_ok(&request("GET", "/ws", "", &[]), Door::Local));
}

#[test]
fn the_page_may_open_a_secure_socket_to_public_hosts() {
    let policy = doors().content_security_policy();
    assert!(policy.contains("wss://herdr.example.com"));
    assert!(policy.contains("ws://localhost:4321"));
    assert!(policy.contains("frame-ancestors 'none'"));
    assert!(policy.contains("worker-src 'self'"));
}
