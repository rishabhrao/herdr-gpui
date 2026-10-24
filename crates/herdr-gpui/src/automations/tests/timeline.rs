//! The timeline's groups and order.
use super::*;
use crate::automations::timeline::{Group, Source, entries};
use chrono::FixedOffset;

#[test]
fn the_timeline_leads_with_what_needs_you_then_now_next_and_past_days() {
    let utc = FixedOffset::east_opt(0).unwrap();
    let status = status();
    let machine = Machine::Local;
    let sources = [Source {
        machine: &machine,
        label: "here",
        status: &status,
    }];
    let entries = entries(&sources, NOW, &utc);
    let groups: Vec<Group> = entries.iter().map(|entry| entry.group).collect();
    let mut sorted = groups.clone();
    sorted.sort();
    assert_eq!(groups, sorted, "groups come in their order");
    let named = |group: Group| -> Vec<(&str, &str)> {
        entries
            .iter()
            .filter(|entry| entry.group == group)
            .map(|entry| (entry.name.as_str(), entry.when.as_str()))
            .collect()
    };
    // The approval first offers Review; the failure opens its run.
    let needs: Vec<_> = entries
        .iter()
        .filter(|entry| entry.group == Group::NeedsYou)
        .collect();
    assert_eq!(needs.len(), 2);
    assert!(needs[0].review && needs[0].name == "deploy");
    assert!(matches!(needs[1].open, Opened::Run { run: 126, .. }));
    assert_eq!(named(Group::Now), [("nightly", "11:48")]);
    // Only active scripts fire; paused and waiting ones have no next.
    assert_eq!(
        named(Group::Next),
        [("nightly", "in 15 h"), ("triage", "in 15 h")]
    );
    // The running run is in Now, not among the past days.
    assert_eq!(named(Group::Today), [("triage", "09:00")]);
    assert_eq!(named(Group::Yesterday), [("nightly", "Sun 03:00")]);
    assert_eq!(named(Group::ThisWeek), [("nightly", "Thu 12:00")]);
    assert_eq!(named(Group::Older), [("nightly", "Sep 29")]);
}

#[test]
fn every_machine_adds_its_own_lines() {
    let utc = FixedOffset::east_opt(0).unwrap();
    let status = status();
    let (here, there) = (Machine::Local, Machine::Ssh("box".into()));
    let one = entries(
        &[Source {
            machine: &here,
            label: "here",
            status: &status,
        }],
        NOW,
        &utc,
    );
    let both = entries(
        &[
            Source {
                machine: &here,
                label: "here",
                status: &status,
            },
            Source {
                machine: &there,
                label: "box",
                status: &status,
            },
        ],
        NOW,
        &utc,
    );
    assert_eq!(both.len(), one.len() * 2);
    assert!(both.iter().any(|entry| entry.host == "box"));
    assert!(
        both.iter()
            .filter(|entry| entry.host == "box")
            .all(|entry| *entry.open.machine() == there)
    );
}
