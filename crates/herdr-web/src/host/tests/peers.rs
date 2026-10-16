use crate::host::peers::{Left, Peers};

#[test]
fn the_first_page_drives_until_another_is_used() {
    let mut peers = Peers::default();
    peers.add(1, false);
    peers.add(2, true);
    assert_eq!(peers.driver(), Some(1));
    assert!(!peers.driver_is_remote());

    peers.set_viewport(2, 7, (400., 800., 3.));
    let phone = peers.drive(2).unwrap();
    assert_eq!(phone.viewports.get(&7), Some(&(400., 800., 3.)));
    assert!(peers.driver_is_remote());
    // Using the driver again changes nothing.
    assert!(peers.drive(2).is_none());
}

#[test]
fn the_page_used_before_drives_when_the_driver_leaves() {
    let mut peers = Peers::default();
    peers.add(1, false);
    peers.add(2, true);
    peers.add(3, false);
    peers.drive(2);
    assert_eq!(peers.remove(1), Left::Other);
    assert_eq!(peers.remove(2), Left::Driver(3));
    assert!(peers.drive(3).is_some());
    assert_eq!(peers.remove(3), Left::Last);
    assert_eq!(peers.remove(3), Left::Unknown);
    assert_eq!(peers.driver(), None);
}

#[test]
fn windows_stay_visible_while_any_page_shows_them() {
    let mut peers = Peers::default();
    peers.add(1, false);
    peers.add(2, true);
    peers.set_visible(1, false);
    assert!(peers.any_visible());
    peers.set_visible(2, false);
    assert!(!peers.any_visible());
}
