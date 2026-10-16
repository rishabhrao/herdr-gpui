//! A finished [`Scene`] as plain data. Every primitive is destructured field
//! by field, without `..`, so a GPUI upgrade that adds a field fails to build
//! here instead of silently dropping it from the page.
use gpui::{
    AtlasTile, Background, BorderStyle, Bounds, ContentMask, Corners, Edges, Hsla,
    MonochromeSprite, PaddedBool32, Path, PathId, PathVertex, PolychromeSprite, Quad, ScaledPixels,
    Scene, Shadow, SubpixelSprite, TransformationMatrix, Underline, point, px, size,
    solid_background,
};
use serde::{Deserialize, Serialize};

/// x, y, width, height.
type Rect = [f32; 4];
/// Top left, top right, bottom right, bottom left.
type Radii = [f32; 4];

/// The atlas a glyph or image tile lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WireTile {
    Mono,
    Subpixel,
    Poly,
}

/// A fill. GPUI's own serde form of [`Background`] stores colors as 8-bit
/// RGBA, so solid fills, nearly all of them, carry their exact color; only
/// gradients and patterns, whose fields are private, take the 8-bit form.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum Fill {
    Solid([f32; 4]),
    Other(Background),
}

impl From<Background> for Fill {
    fn from(background: Background) -> Self {
        match background.as_solid() {
            Some(color) => Self::Solid(hsla(color)),
            None => Self::Other(background),
        }
    }
}

impl From<Fill> for Background {
    fn from(fill: Fill) -> Self {
        match fill {
            Fill::Solid(value) => solid_background(color(value)),
            Fill::Other(background) => background,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WireScene {
    shadows: Vec<WireShadow>,
    quads: Vec<WireQuad>,
    paths: Vec<WirePath>,
    underlines: Vec<WireUnderline>,
    monochrome: Vec<WireSprite>,
    subpixel: Vec<WireSprite>,
    polychrome: Vec<WirePoly>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WireShadow {
    order: u32,
    blur_radius: f32,
    bounds: Rect,
    corner_radii: Radii,
    content_mask: Rect,
    color: [f32; 4],
    element_bounds: Rect,
    element_corner_radii: Radii,
    inset: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WireQuad {
    order: u32,
    border_style: BorderStyle,
    bounds: Rect,
    content_mask: Rect,
    background: Fill,
    border_color: [f32; 4],
    corner_radii: Radii,
    border_widths: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WirePath {
    id: u64,
    order: u32,
    bounds: Rect,
    content_mask: Rect,
    /// Position x, y, then texture s, t.
    vertices: Vec<([f32; 4], Rect)>,
    color: Fill,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WireUnderline {
    order: u32,
    bounds: Rect,
    content_mask: Rect,
    color: [f32; 4],
    thickness: f32,
    wavy: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WireSprite {
    order: u32,
    bounds: Rect,
    content_mask: Rect,
    color: [f32; 4],
    tile: u32,
    /// Rotation-scale row major, then translation.
    transformation: [f32; 6],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WirePoly {
    order: u32,
    grayscale: bool,
    opacity: f32,
    bounds: Rect,
    content_mask: Rect,
    corner_radii: Radii,
    tile: u32,
}

fn rect(bounds: Bounds<ScaledPixels>) -> Rect {
    [
        bounds.origin.x.0,
        bounds.origin.y.0,
        bounds.size.width.0,
        bounds.size.height.0,
    ]
}

fn bounds([x, y, width, height]: Rect) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(ScaledPixels(x), ScaledPixels(y)),
        size: size(ScaledPixels(width), ScaledPixels(height)),
    }
}

fn mask(mask: ContentMask<ScaledPixels>) -> Rect {
    rect(mask.bounds)
}

fn content_mask(value: Rect) -> ContentMask<ScaledPixels> {
    ContentMask {
        bounds: bounds(value),
    }
}

fn hsla(color: Hsla) -> [f32; 4] {
    [color.h, color.s, color.l, color.a]
}

fn color([h, s, l, a]: [f32; 4]) -> Hsla {
    Hsla { h, s, l, a }
}

fn radii(corners: &Corners<ScaledPixels>) -> Radii {
    [
        corners.top_left.0,
        corners.top_right.0,
        corners.bottom_right.0,
        corners.bottom_left.0,
    ]
}

fn corners([top_left, top_right, bottom_right, bottom_left]: Radii) -> Corners<ScaledPixels> {
    Corners {
        top_left: ScaledPixels(top_left),
        top_right: ScaledPixels(top_right),
        bottom_right: ScaledPixels(bottom_right),
        bottom_left: ScaledPixels(bottom_left),
    }
}

fn edges(edges: &Edges<ScaledPixels>) -> [f32; 4] {
    [edges.top.0, edges.right.0, edges.bottom.0, edges.left.0]
}

fn widths([top, right, bottom, left]: [f32; 4]) -> Edges<ScaledPixels> {
    Edges {
        top: ScaledPixels(top),
        right: ScaledPixels(right),
        bottom: ScaledPixels(bottom),
        left: ScaledPixels(left),
    }
}

fn matrix(matrix: TransformationMatrix) -> [f32; 6] {
    let TransformationMatrix {
        rotation_scale: [[a, b], [c, d]],
        translation: [x, y],
    } = matrix;
    [a, b, c, d, x, y]
}

fn transformation([a, b, c, d, x, y]: [f32; 6]) -> TransformationMatrix {
    TransformationMatrix {
        rotation_scale: [[a, b], [c, d]],
        translation: [x, y],
    }
}

fn flag(value: PaddedBool32) -> bool {
    value == PaddedBool32::from(true)
}

impl WireScene {
    /// Copies a finished scene, naming each sprite's atlas tile by its id.
    pub fn from_scene(scene: &Scene) -> Self {
        Self {
            shadows: scene.shadows.iter().map(shadow_to_wire).collect(),
            quads: scene.quads.iter().map(quad_to_wire).collect(),
            paths: scene.paths.iter().map(path_to_wire).collect(),
            underlines: scene.underlines.iter().map(underline_to_wire).collect(),
            monochrome: scene.monochrome_sprites.iter().map(mono_to_wire).collect(),
            subpixel: scene
                .subpixel_sprites
                .iter()
                .map(subpixel_to_wire)
                .collect(),
            polychrome: scene.polychrome_sprites.iter().map(poly_to_wire).collect(),
        }
    }

    /// The scene again, with each tile id swapped for the tile `tile` finds
    /// in this end's atlas. Sprites whose tile is unknown are left out.
    pub fn into_scene(self, mut tile: impl FnMut(WireTile, u32) -> Option<AtlasTile>) -> Scene {
        let mut scene = Scene::default();
        scene.shadows = self.shadows.into_iter().map(shadow_from_wire).collect();
        scene.quads = self.quads.into_iter().map(quad_from_wire).collect();
        scene.paths = self.paths.into_iter().map(path_from_wire).collect();
        scene.underlines = self
            .underlines
            .into_iter()
            .map(underline_from_wire)
            .collect();
        scene.monochrome_sprites = self
            .monochrome
            .into_iter()
            .filter_map(|sprite| {
                let found = tile(WireTile::Mono, sprite.tile)?;
                Some(mono_from_wire(sprite, found))
            })
            .collect();
        scene.subpixel_sprites = self
            .subpixel
            .into_iter()
            .filter_map(|sprite| {
                let found = tile(WireTile::Subpixel, sprite.tile)?;
                Some(subpixel_from_wire(sprite, found))
            })
            .collect();
        scene.polychrome_sprites = self
            .polychrome
            .into_iter()
            .filter_map(|sprite| {
                let found = tile(WireTile::Poly, sprite.tile)?;
                Some(poly_from_wire(sprite, found))
            })
            .collect();
        // The new tiles' ids differ, so restore the order batching expects.
        scene.finish();
        scene
    }

    /// How many primitives the scene holds.
    pub fn len(&self) -> usize {
        self.shadows.len()
            + self.quads.len()
            + self.paths.len()
            + self.underlines.len()
            + self.monochrome.len()
            + self.subpixel.len()
            + self.polychrome.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn shadow_to_wire(shadow: &Shadow) -> WireShadow {
    let Shadow {
        order,
        blur_radius,
        bounds,
        corner_radii,
        content_mask,
        color,
        element_bounds,
        element_corner_radii,
        inset,
        pad: _,
    } = *shadow;
    WireShadow {
        order,
        blur_radius: blur_radius.0,
        bounds: rect(bounds),
        corner_radii: radii(&corner_radii),
        content_mask: mask(content_mask),
        color: hsla(color),
        element_bounds: rect(element_bounds),
        element_corner_radii: radii(&element_corner_radii),
        inset,
    }
}

fn shadow_from_wire(shadow: WireShadow) -> Shadow {
    Shadow {
        order: shadow.order,
        blur_radius: ScaledPixels(shadow.blur_radius),
        bounds: bounds(shadow.bounds),
        corner_radii: corners(shadow.corner_radii),
        content_mask: content_mask(shadow.content_mask),
        color: color(shadow.color),
        element_bounds: bounds(shadow.element_bounds),
        element_corner_radii: corners(shadow.element_corner_radii),
        inset: shadow.inset,
        pad: 0,
    }
}

fn quad_to_wire(quad: &Quad) -> WireQuad {
    let Quad {
        order,
        border_style,
        bounds,
        content_mask,
        background,
        border_color,
        corner_radii,
        border_widths,
    } = quad;
    WireQuad {
        order: *order,
        border_style: *border_style,
        bounds: rect(*bounds),
        content_mask: mask(*content_mask),
        background: (*background).into(),
        border_color: hsla(*border_color),
        corner_radii: radii(corner_radii),
        border_widths: edges(border_widths),
    }
}

fn quad_from_wire(quad: WireQuad) -> Quad {
    Quad {
        order: quad.order,
        border_style: quad.border_style,
        bounds: bounds(quad.bounds),
        content_mask: content_mask(quad.content_mask),
        background: quad.background.into(),
        border_color: color(quad.border_color),
        corner_radii: corners(quad.corner_radii),
        border_widths: widths(quad.border_widths),
    }
}

fn path_to_wire(path: &Path<ScaledPixels>) -> WirePath {
    WirePath {
        id: path.id.0 as u64,
        order: path.order,
        bounds: rect(path.bounds),
        content_mask: mask(path.content_mask),
        vertices: path
            .vertices
            .iter()
            .map(|vertex| {
                let PathVertex {
                    xy_position,
                    st_position,
                    content_mask,
                } = vertex;
                (
                    [
                        xy_position.x.0,
                        xy_position.y.0,
                        st_position.x,
                        st_position.y,
                    ],
                    mask(*content_mask),
                )
            })
            .collect(),
        color: path.color.into(),
    }
}

fn path_from_wire(path: WirePath) -> Path<ScaledPixels> {
    // A path's private builder state only matters while it is drawn out.
    let mut built = Path::new(point(px(0.), px(0.))).scale(1.);
    built.id = PathId(path.id as usize);
    built.order = path.order;
    built.bounds = bounds(path.bounds);
    built.content_mask = content_mask(path.content_mask);
    built.color = path.color.into();
    built.vertices = path
        .vertices
        .into_iter()
        .map(|([x, y, s, t], vertex_mask)| PathVertex {
            xy_position: point(ScaledPixels(x), ScaledPixels(y)),
            st_position: point(s, t),
            content_mask: content_mask(vertex_mask),
        })
        .collect();
    built
}

fn underline_to_wire(underline: &Underline) -> WireUnderline {
    let Underline {
        order,
        pad: _,
        bounds,
        content_mask,
        color,
        thickness,
        wavy,
    } = *underline;
    WireUnderline {
        order,
        bounds: rect(bounds),
        content_mask: mask(content_mask),
        color: hsla(color),
        thickness: thickness.0,
        wavy: flag(wavy),
    }
}

fn underline_from_wire(underline: WireUnderline) -> Underline {
    Underline {
        order: underline.order,
        pad: 0,
        bounds: bounds(underline.bounds),
        content_mask: content_mask(underline.content_mask),
        color: color(underline.color),
        thickness: ScaledPixels(underline.thickness),
        wavy: underline.wavy.into(),
    }
}

fn mono_to_wire(sprite: &MonochromeSprite) -> WireSprite {
    let MonochromeSprite {
        order,
        pad: _,
        bounds,
        content_mask,
        color,
        tile,
        transformation,
    } = *sprite;
    WireSprite {
        order,
        bounds: rect(bounds),
        content_mask: mask(content_mask),
        color: hsla(color),
        tile: tile.tile_id.0,
        transformation: matrix(transformation),
    }
}

fn mono_from_wire(sprite: WireSprite, tile: AtlasTile) -> MonochromeSprite {
    MonochromeSprite {
        order: sprite.order,
        pad: 0,
        bounds: bounds(sprite.bounds),
        content_mask: content_mask(sprite.content_mask),
        color: color(sprite.color),
        tile,
        transformation: transformation(sprite.transformation),
    }
}

fn subpixel_to_wire(sprite: &SubpixelSprite) -> WireSprite {
    let SubpixelSprite {
        order,
        pad: _,
        bounds,
        content_mask,
        color,
        tile,
        transformation,
    } = *sprite;
    WireSprite {
        order,
        bounds: rect(bounds),
        content_mask: mask(content_mask),
        color: hsla(color),
        tile: tile.tile_id.0,
        transformation: matrix(transformation),
    }
}

fn subpixel_from_wire(sprite: WireSprite, tile: AtlasTile) -> SubpixelSprite {
    SubpixelSprite {
        order: sprite.order,
        pad: 0,
        bounds: bounds(sprite.bounds),
        content_mask: content_mask(sprite.content_mask),
        color: color(sprite.color),
        tile,
        transformation: transformation(sprite.transformation),
    }
}

fn poly_to_wire(sprite: &PolychromeSprite) -> WirePoly {
    let PolychromeSprite {
        order,
        pad: _,
        grayscale,
        opacity,
        bounds,
        content_mask,
        corner_radii,
        tile,
    } = *sprite;
    WirePoly {
        order,
        grayscale: flag(grayscale),
        opacity,
        bounds: rect(bounds),
        content_mask: mask(content_mask),
        corner_radii: radii(&corner_radii),
        tile: tile.tile_id.0,
    }
}

fn poly_from_wire(sprite: WirePoly, tile: AtlasTile) -> PolychromeSprite {
    PolychromeSprite {
        order: sprite.order,
        pad: 0,
        grayscale: sprite.grayscale.into(),
        opacity: sprite.opacity,
        bounds: bounds(sprite.bounds),
        content_mask: content_mask(sprite.content_mask),
        corner_radii: corners(sprite.corner_radii),
        tile,
    }
}
