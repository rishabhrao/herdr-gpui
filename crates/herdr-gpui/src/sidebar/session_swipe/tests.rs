use super::*;

#[test]
fn a_sideways_swipe_steps_once_however_far_it_goes() {
    let mut swipe = SessionSwipe::default();
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    assert_eq!(swipe.scroll((-40., 2.), at(0)), None);
    assert_eq!(swipe.scroll((-60., 0.), at(16)), Some(Step::Next));
    // Coasting on: still the same swipe.
    assert_eq!(swipe.scroll((-200., 0.), at(32)), None);
    // After a pause, a swipe the other way.
    assert_eq!(swipe.scroll((100., 0.), at(400)), Some(Step::Previous));
}

#[test]
fn a_mostly_vertical_scroll_is_not_a_swipe() {
    let mut swipe = SessionSwipe::default();
    let now = Instant::now();
    for _ in 0..10 {
        assert_eq!(swipe.scroll((-30., 40.), now), None);
    }
}

#[test]
fn the_ends_do_not_wrap() {
    let sessions = ["default", "fun"];
    assert_eq!(neighbour(&sessions, Some(0), Step::Next), Some(1));
    assert_eq!(neighbour(&sessions, Some(1), Step::Next), None);
    assert_eq!(neighbour(&sessions, Some(0), Step::Previous), None);
    assert_eq!(neighbour(&sessions, None, Step::Next), None);
    assert_eq!(dots(3, 1), "○ ● ○");
}
