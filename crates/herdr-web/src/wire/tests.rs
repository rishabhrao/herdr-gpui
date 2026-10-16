#![allow(clippy::unwrap_used)]
use super::*;
use gpui::{
    AtlasTextureId, AtlasTextureKind, AtlasTile, BorderStyle, Bounds, ContentMask, Corners,
    DevicePixels, Edges, MonochromeSprite, Path, PolychromeSprite, Quad, ScaledPixels, Scene,
    Shadow, SubpixelSprite, TileId, TransformationMatrix, Underline, hsla, linear_color_stop,
    linear_gradient, point, px, size, solid_background,
};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(ScaledPixels(x), ScaledPixels(y)),
        size: size(ScaledPixels(w), ScaledPixels(h)),
    }
}

fn mask() -> ContentMask<ScaledPixels> {
    ContentMask {
        bounds: rect(0., 0., 800., 600.),
    }
}

fn tile(kind: AtlasTextureKind, id: u32) -> AtlasTile {
    AtlasTile {
        texture_id: AtlasTextureId { index: 0, kind },
        tile_id: TileId(id),
        padding: 0,
        bounds: Bounds {
            origin: point(DevicePixels(0), DevicePixels(0)),
            size: size(DevicePixels(8), DevicePixels(12)),
        },
    }
}

fn corners(radius: f32) -> Corners<ScaledPixels> {
    Corners {
        top_left: ScaledPixels(radius),
        top_right: ScaledPixels(radius + 1.),
        bottom_right: ScaledPixels(radius + 2.),
        bottom_left: ScaledPixels(radius + 3.),
    }
}

/// One of every primitive, through the same public calls GPUI paints with.
fn every_primitive() -> Scene {
    let mut scene = Scene::default();
    scene.insert_primitive(Shadow {
        order: 0,
        blur_radius: ScaledPixels(4.),
        bounds: rect(10., 10., 100., 50.),
        corner_radii: corners(2.),
        content_mask: mask(),
        color: hsla(0.1, 0.2, 0.3, 0.4),
        element_bounds: rect(12., 12., 96., 46.),
        element_corner_radii: corners(1.),
        inset: 1,
        pad: 0,
    });
    scene.push_layer(rect(0., 0., 400., 300.));
    scene.insert_primitive(Quad {
        order: 0,
        border_style: BorderStyle::Dashed,
        bounds: rect(20., 30., 40., 50.),
        content_mask: mask(),
        background: solid_background(hsla(0.55, 0.45, 0.35, 0.9)),
        border_color: hsla(0.9, 0.1, 0.2, 1.),
        corner_radii: corners(3.),
        border_widths: Edges {
            top: ScaledPixels(1.),
            right: ScaledPixels(2.),
            bottom: ScaledPixels(3.),
            left: ScaledPixels(4.),
        },
    });
    let mut path = Path::new(point(px(1.), px(2.)));
    path.line_to(point(px(30.), px(2.)));
    path.line_to(point(px(30.), px(40.)));
    path.color = solid_background(hsla(0.3, 0.3, 0.3, 1.));
    let mut path = path.scale(2.);
    path.content_mask = mask();
    scene.insert_primitive(path);
    scene.insert_primitive(Underline {
        order: 0,
        pad: 0,
        bounds: rect(5., 60., 50., 2.),
        content_mask: mask(),
        color: hsla(0.2, 0.9, 0.5, 1.),
        thickness: ScaledPixels(1.5),
        wavy: true.into(),
    });
    scene.pop_layer();
    scene.insert_primitive(MonochromeSprite {
        order: 0,
        pad: 0,
        bounds: rect(70., 80., 8., 12.),
        content_mask: mask(),
        color: hsla(0., 0., 1., 1.),
        tile: tile(AtlasTextureKind::Monochrome, 7),
        transformation: TransformationMatrix::unit()
            .translate(point(ScaledPixels(3.), ScaledPixels(4.))),
    });
    scene.insert_primitive(SubpixelSprite {
        order: 0,
        pad: 0,
        bounds: rect(90., 80., 8., 12.),
        content_mask: mask(),
        color: hsla(0., 0., 0.5, 1.),
        tile: tile(AtlasTextureKind::Subpixel, 8),
        transformation: TransformationMatrix::unit(),
    });
    scene.insert_primitive(PolychromeSprite {
        order: 0,
        pad: 0,
        grayscale: true.into(),
        opacity: 0.75,
        bounds: rect(110., 80., 16., 16.),
        content_mask: mask(),
        corner_radii: corners(4.),
        tile: tile(AtlasTextureKind::Polychrome, 9),
    });
    scene.finish();
    scene
}

fn same_tile(kind: WireTile, id: u32) -> Option<AtlasTile> {
    let kind = match kind {
        WireTile::Mono => AtlasTextureKind::Monochrome,
        WireTile::Subpixel => AtlasTextureKind::Subpixel,
        WireTile::Poly => AtlasTextureKind::Polychrome,
    };
    Some(tile(kind, id))
}

#[test]
fn every_primitive_survives_the_wire() {
    let scene = every_primitive();
    let wire = WireScene::from_scene(&scene);
    assert_eq!(wire.len(), 7);
    let message = ToBrowser::Frame {
        window: 3,
        seq: 9,
        scene: wire.clone(),
    };
    let decoded: ToBrowser = decode(&encode(&message).unwrap()).unwrap();
    assert_eq!(decoded, message);
    let ToBrowser::Frame { scene: decoded, .. } = decoded else {
        unreachable!()
    };
    let rebuilt = decoded.into_scene(same_tile);
    assert_eq!(WireScene::from_scene(&rebuilt), wire);
    // Draw order and layering are what the host computed, not recomputed.
    assert_eq!(
        rebuilt.quads[0].order, scene.quads[0].order,
        "a layer's primitives keep the layer's order"
    );
    assert_eq!(
        rebuilt.paths[0].vertices.len(),
        scene.paths[0].vertices.len()
    );
    assert_eq!(rebuilt.batches().count(), scene.batches().count());
}

#[test]
fn sprites_take_this_ends_tiles_and_unknown_ones_are_dropped() {
    let wire = WireScene::from_scene(&every_primitive());
    let rebuilt = wire.into_scene(|kind, id| {
        (kind == WireTile::Mono).then(|| tile(AtlasTextureKind::Monochrome, id + 100))
    });
    assert_eq!(rebuilt.monochrome_sprites.len(), 1);
    assert_eq!(rebuilt.monochrome_sprites[0].tile.tile_id, TileId(107));
    assert!(rebuilt.subpixel_sprites.is_empty());
    assert!(rebuilt.polychrome_sprites.is_empty());
}

#[test]
fn messages_round_trip() {
    let messages = [
        ToHost::Hello { protocol: PROTOCOL },
        ToHost::Clipboard {
            text: "copied on the phone".into(),
        },
        ToHost::Input {
            window: 1,
            input: Input::KeyDown {
                key: "a".into(),
                key_char: Some("å".into()),
                mods: Mods {
                    alt: true,
                    ..Mods::default()
                },
                held: false,
            },
        },
        ToHost::Input {
            window: 1,
            input: Input::Scroll {
                x: 1.,
                y: 2.,
                delta: Delta::Lines(0., -3.),
                mods: Mods::default(),
            },
        },
        ToHost::Text {
            window: 2,
            op: TextOp::Compose {
                text: "にほ".into(),
                selected: Some((2, 2)),
            },
        },
        ToHost::Page(PageEvent::Posted {
            page: 4,
            body: "{}".into(),
        }),
    ];
    for message in messages {
        let decoded: ToHost = decode(&encode(&message).unwrap()).unwrap();
        assert_eq!(decoded, message);
    }
}

#[test]
fn trailing_bytes_and_truncation_are_refused() {
    let mut bytes = encode(&ToHost::Tick { window: 1 }).unwrap();
    bytes.push(0);
    assert!(matches!(
        decode::<ToHost>(&bytes),
        Err(crate::Error::Trailing(1))
    ));
    let bytes = encode(&ToBrowser::Title {
        window: 1,
        title: "Herdr".into(),
    })
    .unwrap();
    assert!(matches!(
        decode::<ToBrowser>(&bytes[..bytes.len() - 2]),
        Err(crate::Error::Decode(_))
    ));
}

#[test]
fn a_huge_claimed_length_is_refused_without_allocating_it() {
    // An `Upload` whose byte vector claims u64::MAX entries.
    let mut bytes = encode(&ToBrowser::Upload {
        window: 1,
        tiles: vec![TileUpload {
            id: 1,
            kind: WireTile::Mono,
            width: 1,
            height: 1,
            bytes: Vec::new(),
        }],
    })
    .unwrap();
    bytes.pop();
    bytes.extend([253, 255, 255, 255, 255, 255, 255, 255, 255]);
    assert!(decode::<ToBrowser>(&bytes).is_err());
}

#[test]
fn solid_fills_are_exact_and_gradients_keep_eight_bits() {
    let mut scene = Scene::default();
    let quad = |background| Quad {
        order: 0,
        border_style: BorderStyle::Solid,
        bounds: rect(0., 0., 10., 10.),
        content_mask: mask(),
        background,
        border_color: hsla(0., 0., 0., 0.),
        corner_radii: corners(0.),
        border_widths: Edges::default(),
    };
    let exact = hsla(0.123_456, 0.654_321, 0.333_333, 0.777_777);
    scene.insert_primitive(quad(solid_background(exact)));
    scene.insert_primitive(quad(linear_gradient(
        45.,
        linear_color_stop(hsla(0.5, 0.5, 0.5, 1.), 0.),
        linear_color_stop(hsla(0.7, 0.5, 0.5, 1.), 1.),
    )));
    scene.finish();
    let bytes = encode(&WireScene::from_scene(&scene)).unwrap();
    let rebuilt = decode::<WireScene>(&bytes).unwrap().into_scene(same_tile);
    assert_eq!(rebuilt.quads[0].background.as_solid(), Some(exact));
    let gradient = format!("{:?}", rebuilt.quads[1].background);
    assert!(gradient.starts_with("LinearGradient(45"), "{gradient}");
}
