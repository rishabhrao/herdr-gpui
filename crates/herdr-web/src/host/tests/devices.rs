use super::*;
use crate::host::devices::{self, Claim, Refusal, clean_label};
use std::os::unix::fs::PermissionsExt as _;

#[test]
fn a_device_is_known_by_its_token_and_only_its_hash_is_kept() {
    let dir = Dir::new("devices");
    let registry = Devices::new(&dir.0);
    let (device, token) = registry.add("My Phone", true).unwrap();
    assert_eq!(registry.check(&token).unwrap().id, device.id);
    assert!(registry.check(&random_hex(32).unwrap()).is_none());
    let file = std::fs::read_to_string(dir.0.join(devices::FILE)).unwrap();
    assert!(!file.contains(&token), "the token itself is never stored");
    let mode = std::fs::metadata(dir.0.join(devices::FILE))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn forgetting_a_device_takes_effect_on_a_running_server() {
    let dir = Dir::new("forget");
    let registry = Devices::new(&dir.0);
    let (device, token) = registry.add("Old phone", true).unwrap();
    // The CLI edits the file; the server reads it again on the next check.
    assert_eq!(devices::forget(&dir.0, "Old phone").unwrap(), 1);
    assert!(registry.check(&token).is_none());
    assert!(devices::list(&dir.0).is_empty());
    assert!(!registry.forget(&device.id).unwrap());
}

#[test]
fn a_broken_registry_pairs_nothing() {
    let dir = Dir::new("broken");
    let registry = Devices::new(&dir.0);
    let (_, token) = registry.add("Phone", true).unwrap();
    std::fs::write(dir.0.join(devices::FILE), b"{ not json").unwrap();
    assert!(registry.check(&token).is_none());
}

#[test]
fn an_approved_request_hands_its_token_over_once() {
    let dir = Dir::new("approve");
    let registry = Devices::new(&dir.0);
    let (request, secret) = registry.ask("iPhone Safari", true).unwrap().unwrap();
    assert_eq!(request.code.len(), 7);
    assert_eq!(registry.claim(request.id, &secret), Claim::Waiting);
    // Someone else's browser, without the secret, learns nothing.
    assert_eq!(registry.claim(request.id, "guess"), Claim::Gone);
    assert!(registry.answer(request.id, true).unwrap());
    let Claim::Approved(token) = registry.claim(request.id, &secret) else {
        panic!("approved");
    };
    assert_eq!(registry.check(&token).unwrap().label, "iPhone Safari");
    assert!(registry.check(&token).unwrap().remote);
    assert_eq!(registry.claim(request.id, &secret), Claim::Gone);
    assert!(!registry.answer(request.id, true).unwrap());
}

#[test]
fn a_denied_request_pairs_nothing() {
    let dir = Dir::new("deny");
    let registry = Devices::new(&dir.0);
    let (request, secret) = registry.ask("Stranger", true).unwrap().unwrap();
    assert!(registry.answer(request.id, false).unwrap());
    assert_eq!(registry.claim(request.id, &secret), Claim::Denied);
    assert!(registry.list().is_empty());
}

#[test]
fn requests_are_bounded() {
    let dir = Dir::new("bounded");
    let registry = Devices::new(&dir.0);
    for _ in 0..4 {
        assert!(registry.ask("Phone", true).unwrap().is_ok());
    }
    assert_eq!(
        registry.ask("Phone", true).unwrap().unwrap_err(),
        Refusal::Busy
    );
    let mut refusal = None;
    for _ in 0..12 {
        if let Err(error) = registry.ask("Phone", true).unwrap() {
            refusal = Some(error);
        }
    }
    assert_eq!(refusal, Some(Refusal::TooMany));
}

#[test]
fn labels_are_printable_and_bounded() {
    assert_eq!(clean_label("  Phone\u{7}\n "), "Phone");
    assert_eq!(clean_label(""), "A browser");
    assert_eq!(clean_label(&"x".repeat(200)).chars().count(), 48);
}
