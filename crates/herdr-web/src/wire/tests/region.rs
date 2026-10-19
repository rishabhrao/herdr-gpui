use super::*;
use crate::wire::region::{Rect, overlap, uncovered};

fn area(pieces: &[Rect]) -> f32 {
    pieces.iter().map(|piece| piece[2] * piece[3]).sum()
}

#[test]
fn rectangles_that_share_an_edge_do_not_overlap() {
    assert_eq!(overlap(&[0., 0., 10., 10.], &[10., 0., 10., 10.]), None);
    assert_eq!(overlap(&[0., 0., 10., 10.], &[9.8, 0., 10., 10.]), None);
    assert_eq!(
        overlap(&[0., 0., 10., 10.], &[5., 5., 10., 10.]),
        Some([5., 5., 5., 5.])
    );
}

#[test]
fn a_page_nothing_covers_shows_whole() {
    let page = [0., 100., 400., 600.];
    assert_eq!(uncovered(page, Vec::new()), vec![page]);
    // The ground painted around the page only meets its edges.
    let around = vec![[0., 0., 400., 100.], [0., 700., 400., 50.]];
    assert_eq!(uncovered(page, around), vec![page]);
}

#[test]
fn a_drawer_over_a_page_leaves_the_rest_of_it() {
    let page = [0., 100., 400., 600.];
    let drawer = [0., 0., 320., 800.];
    let pieces = uncovered(page, vec![drawer]);
    assert_eq!(pieces, vec![[320., 100., 80., 600.]]);
}

#[test]
fn a_panel_and_its_text_cover_one_box() {
    let page = [0., 0., 400., 400.];
    // A menu's ground, with glyphs on it and one just past its edge.
    let mut covers = vec![[100., 100., 100., 100.]];
    covers.extend((0..20).map(|index| [104. + index as f32 * 5., 110., 4., 10.]));
    let pieces = uncovered(page, covers);
    assert!(pieces.len() <= 4, "{pieces:?}");
    let covered = 400. * 400. - area(&pieces);
    assert!(covered >= 100. * 100., "the menu is covered: {covered}");
    assert!(
        covered <= 110. * 110.,
        "little more than the menu: {covered}"
    );
    for piece in &pieces {
        assert!(overlap(piece, &[100., 100., 100., 100.]).is_none());
    }
}

#[test]
fn the_pieces_left_never_overlap() {
    let page = [0., 0., 300., 300.];
    let covers = vec![
        [10., 10., 50., 50.],
        [200., 20., 30., 200.],
        [40., 250., 100., 20.],
    ];
    let pieces = uncovered(page, covers.clone());
    for (index, piece) in pieces.iter().enumerate() {
        for other in &pieces[index + 1..] {
            assert!(overlap(piece, other).is_none(), "{piece:?} {other:?}");
        }
        for cover in &covers {
            assert!(overlap(piece, cover).is_none(), "{piece:?} under {cover:?}");
        }
    }
    let covered: f32 = covers.iter().map(|cover| cover[2] * cover[3]).sum();
    assert!((area(&pieces) + covered - 300. * 300.).abs() < 1.);
}

#[test]
fn many_scattered_covers_cover_their_bounding_box() {
    let page = [0., 0., 1000., 1000.];
    let covers = (0..100)
        .map(|index| {
            [
                (index % 10) as f32 * 90.,
                (index / 10) as f32 * 90.,
                10.,
                10.,
            ]
        })
        .collect();
    let pieces = uncovered(page, covers);
    assert!(pieces.len() <= 4, "{pieces:?}");
}

#[test]
fn what_the_window_paints_over_a_page_is_found() {
    let scene = WireScene::from_scene(&every_primitive());
    // Away from every primitive: nothing covers it.
    assert!(scene.painted_over(&[600., 400., 100., 100.]).is_empty());
    // The quad at 20,30 40x50 reaches into this area, cut to it.
    let found = scene.painted_over(&[40., 40., 10., 10.]);
    assert!(found.contains(&[40., 40., 10., 10.]), "{found:?}");
    // A primitive's content mask hides what lies outside it.
    let mut scene = Scene::default();
    scene.insert_primitive(Quad {
        order: 0,
        border_style: BorderStyle::Solid,
        bounds: rect(0., 0., 100., 100.),
        content_mask: ContentMask {
            bounds: rect(0., 0., 50., 100.),
        },
        background: solid_background(hsla(0., 0., 0., 1.)),
        border_color: hsla(0., 0., 0., 0.),
        corner_radii: corners(0.),
        border_widths: Edges::default(),
    });
    // Nor does a quad that paints nothing.
    scene.insert_primitive(Quad {
        order: 0,
        border_style: BorderStyle::Solid,
        bounds: rect(0., 0., 100., 100.),
        content_mask: mask(),
        background: solid_background(hsla(0., 0., 0., 0.)),
        border_color: hsla(0., 0., 0., 1.),
        corner_radii: corners(0.),
        border_widths: Edges::default(),
    });
    scene.finish();
    let wire = WireScene::from_scene(&scene);
    assert_eq!(
        wire.painted_over(&[0., 0., 100., 100.]),
        vec![[0., 0., 50., 100.]]
    );
}
