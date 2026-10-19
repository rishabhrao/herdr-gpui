//! What still shows of a page that lies beneath the window's drawing: its
//! area less everything the window paints over it, such as a menu, a
//! drawer, or a toast. The page takes taps there and nowhere else, as a
//! native page does under the window's own hit test.

/// x, y, width, height.
pub type Rect = [f32; 4];

/// Rectangles that overlap by less than this, in device pixels, only share
/// an edge, as the window's ground painted around a page shares the page's.
const EDGE: f32 = 0.5;
/// Covers this close are one: a panel's text and icons lie on its ground,
/// and the gaps between letters cover nothing worth reaching.
const GAP: f32 = 4.;
/// More separate covers than this, after merging, cover their bounding box:
/// it keeps the page's shape, and the work, small.
const MAX_COVERS: usize = 32;

fn right(rect: &Rect) -> f32 {
    rect[0] + rect[2]
}

fn bottom(rect: &Rect) -> f32 {
    rect[1] + rect[3]
}

fn corners(left: f32, top: f32, right: f32, bottom: f32) -> Rect {
    [left, top, right - left, bottom - top]
}

/// Where `a` and `b` overlap, if by more than an edge.
pub fn overlap(a: &Rect, b: &Rect) -> Option<Rect> {
    let (left, top) = (a[0].max(b[0]), a[1].max(b[1]));
    let (right, bottom) = (right(a).min(right(b)), bottom(a).min(bottom(b)));
    (right - left > EDGE && bottom - top > EDGE).then(|| corners(left, top, right, bottom))
}

fn near(a: &Rect, b: &Rect) -> bool {
    a[0] - GAP < right(b)
        && b[0] - GAP < right(a)
        && a[1] - GAP < bottom(b)
        && b[1] - GAP < bottom(a)
}

fn union(a: &Rect, b: &Rect) -> Rect {
    corners(
        a[0].min(b[0]),
        a[1].min(b[1]),
        right(a).max(right(b)),
        bottom(a).max(bottom(b)),
    )
}

/// `covers` as boxes that are not near one another.
fn merge(mut covers: Vec<Rect>) -> Vec<Rect> {
    let mut changed = true;
    while changed {
        changed = false;
        let mut index = 0;
        while index < covers.len() {
            let mut other = index + 1;
            while other < covers.len() {
                if near(&covers[index], &covers[other]) {
                    let taken = covers.swap_remove(other);
                    covers[index] = union(&covers[index], &taken);
                    changed = true;
                } else {
                    other += 1;
                }
            }
            index += 1;
        }
    }
    if covers.len() > MAX_COVERS
        && let Some(first) = covers.first().copied()
    {
        return vec![covers.iter().fold(first, |all, cover| union(&all, cover))];
    }
    covers
}

/// `piece` less `cover`: the bands above, below, left, and right of where
/// they overlap, or `piece` itself where they do not.
fn outside(piece: Rect, cover: &Rect) -> Vec<Rect> {
    let Some(cut) = overlap(&piece, cover) else {
        return vec![piece];
    };
    let mut bands = Vec::with_capacity(4);
    if cut[1] > piece[1] {
        bands.push(corners(piece[0], piece[1], right(&piece), cut[1]));
    }
    if bottom(&cut) < bottom(&piece) {
        bands.push(corners(
            piece[0],
            bottom(&cut),
            right(&piece),
            bottom(&piece),
        ));
    }
    if cut[0] > piece[0] {
        bands.push(corners(piece[0], cut[1], cut[0], bottom(&cut)));
    }
    if right(&cut) < right(&piece) {
        bands.push(corners(right(&cut), cut[1], right(&piece), bottom(&cut)));
    }
    bands
}

/// `area` less `covers`, as rectangles that do not overlap: where a page in
/// `area` still shows.
pub fn uncovered(area: Rect, covers: Vec<Rect>) -> Vec<Rect> {
    let covers: Vec<Rect> = covers
        .iter()
        .filter_map(|cover| overlap(&area, cover))
        .collect();
    let mut pieces = vec![area];
    for cover in merge(covers) {
        pieces = pieces
            .into_iter()
            .flat_map(|piece| outside(piece, &cover))
            .collect();
    }
    pieces.retain(|piece| piece[2] > EDGE && piece[3] > EDGE);
    pieces
}
