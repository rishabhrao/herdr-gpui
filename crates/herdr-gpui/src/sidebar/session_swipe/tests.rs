use super::*;

#[test]
fn a_sideways_swipe_steps_once_however_far_it_coasts() {
    let mut swipe = SessionSwipe::default();
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    assert_eq!(swipe.scroll((-40., 2.), false, at(0)), None);
    assert_eq!(swipe.scroll((-40., 0.), false, at(16)), Some(Step::Next));
    // Coasting on: the same swipe.
    assert_eq!(swipe.scroll((-200., 0.), false, at(32)), None);
    assert_eq!(swipe.scroll((-30., 0.), false, at(48)), None);
}

#[test]
fn a_second_swipe_steps_again_without_a_pause() {
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    // The coasting slows down, and the hand swipes again.
    let mut swipe = SessionSwipe::default();
    assert_eq!(swipe.scroll((-80., 0.), false, at(0)), Some(Step::Next));
    assert_eq!(swipe.scroll((-20., 0.), false, at(16)), None);
    assert_eq!(swipe.scroll((-2., 0.), false, at(32)), None);
    assert_eq!(swipe.scroll((-40., 0.), false, at(48)), None);
    assert_eq!(swipe.scroll((-40., 0.), false, at(64)), Some(Step::Next));
    // A swipe back turns at once.
    assert_eq!(swipe.scroll((80., 0.), false, at(80)), Some(Step::Previous));
    // Fingers coming down start a swipe, coasting or not.
    assert_eq!(swipe.scroll((90., 0.), false, at(96)), None);
    assert_eq!(swipe.scroll((0., 0.), true, at(112)), None);
    assert_eq!(swipe.scroll((80., 0.), false, at(128)), Some(Step::Previous));
}

#[test]
fn a_mostly_vertical_scroll_is_not_a_swipe() {
    let mut swipe = SessionSwipe::default();
    let now = Instant::now();
    for _ in 0..10 {
        assert_eq!(swipe.scroll((-30., 40.), false, now), None);
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
