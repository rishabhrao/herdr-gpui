use super::*;
use crate::{
    host::hub::{Outbound, Picture, Visitor},
    wire::{self, ToBrowser},
};
use std::sync::mpsc;

fn visitor(device: &str) -> Visitor {
    Visitor {
        device: device.into(),
        label: device.into(),
        remote: false,
    }
}

fn received(receiver: &mpsc::Receiver<Outbound>) -> Vec<ToBrowser> {
    receiver
        .try_iter()
        .map(|outbound| wire::decode(&outbound.bytes).unwrap())
        .collect()
}

#[test]
fn every_device_is_served_at_once() {
    let fixture = fixture("hub-many");
    let hub = &fixture.hub;
    let (laptop, laptop_queue) = mpsc::sync_channel(16);
    let (phone, phone_queue) = mpsc::sync_channel(16);
    let laptop = hub.attach(laptop, visitor("laptop")).unwrap();
    let phone = hub.attach(phone, visitor("phone")).unwrap();
    hub.ready(laptop);

    // A page being caught up gets only what is sent to it alone.
    let toast = ToBrowser::Toast { text: "hi".into() };
    hub.send(&toast);
    hub.send_to(phone, &ToBrowser::Hello { protocol: 5 });
    assert_eq!(received(&laptop_queue), std::slice::from_ref(&toast));
    assert_eq!(received(&phone_queue), [ToBrowser::Hello { protocol: 5 }]);

    hub.ready(phone);
    hub.send(&toast);
    assert_eq!(received(&laptop_queue), std::slice::from_ref(&toast));
    assert_eq!(received(&phone_queue), [toast]);

    hub.drop_device("phone");
    assert!(hub.is_connected());
    hub.detach(laptop);
    assert!(!hub.is_connected());
}

#[test]
fn frames_name_their_window_and_a_full_page_is_cut_off_alone() {
    let fixture = fixture("hub-full");
    let hub = &fixture.hub;
    let (slow, slow_queue) = mpsc::sync_channel(1);
    let (fast, fast_queue) = mpsc::sync_channel(16);
    let slow = hub.attach(slow, visitor("slow")).unwrap();
    let fast = hub.attach(fast, visitor("fast")).unwrap();
    hub.ready(slow);
    hub.ready(fast);
    hub.send(&ToBrowser::Frame {
        window: 9,
        seq: 1,
        scene: Default::default(),
    });
    hub.send(&ToBrowser::Toast { text: "x".into() });
    let first = slow_queue.try_recv().unwrap();
    assert_eq!(first.frame, Some(Picture::Window(9)));
    // The slow page's queue was full: it was cut off, the other stays.
    assert!(slow_queue.try_recv().is_err());
    assert_eq!(received(&fast_queue).len(), 2);
    hub.send(&ToBrowser::Toast { text: "y".into() });
    assert_eq!(received(&fast_queue).len(), 1);
}

#[test]
fn too_many_pages_are_refused() {
    let fixture = fixture("hub-limit");
    let hub = &fixture.hub;
    let mut queues = Vec::new();
    let refused = (0..64).find(|index| {
        let (sender, receiver) = mpsc::sync_channel(1);
        queues.push(receiver);
        hub.attach(sender, visitor(&format!("page-{index}")))
            .is_none()
    });
    assert!(refused.is_some_and(|index| index > 1));
}
