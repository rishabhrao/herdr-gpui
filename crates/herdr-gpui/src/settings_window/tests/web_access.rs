use super::*;

#[test]
fn last_seen_reads_in_plain_units() {
    let now = 1_000_000;
    assert_eq!(pages::seen(now - 5, now), "Seen just now");
    assert_eq!(pages::seen(now - 180, now), "Seen 3 min ago");
    assert_eq!(pages::seen(now - 7_200, now), "Seen 2 h ago");
    assert_eq!(pages::seen(now - 3 * 86_400, now), "Seen 3 days ago");
    // A clock that went back never shows a negative time.
    assert_eq!(pages::seen(now + 60, now), "Seen just now");
}

#[test]
fn web_access_sits_with_the_other_connections() {
    assert_eq!(Section::WebAccess.group(), Some(Group::Connect));
    assert!(Section::ALL.contains(&Section::WebAccess));
}
