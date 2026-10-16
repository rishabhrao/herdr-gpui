//! One app window in the page: its canvas, renderer, and the tiles the app
//! uploaded for it. The main window fills the page; others float above it.
use crate::dom;
use gpui::{
    AtlasKey, AtlasTile, DevicePixels, FontId, GlyphId, ImageId, Point, RenderGlyphParams,
    RenderImageParams, RenderSvgParams, Scene, px, size,
};
use gpui_wgpu::{WgpuContext, WgpuRenderer, WgpuSurfaceConfig, wgpu};
use herdr_web::wire::{TileUpload, ToHost, WindowId, WireScene, WireTile};
use std::{borrow::Cow, collections::HashMap};
use web_sys::{HtmlButtonElement, HtmlCanvasElement, HtmlElement};

/// The floating window's title bar, in CSS pixels.
pub(crate) const TITLE_HEIGHT: f64 = 28.;

pub(crate) struct Surface {
    pub(crate) id: WindowId,
    pub(crate) main: bool,
    /// The whole window: the page's main area, or a floating panel.
    pub(crate) element: HtmlElement,
    /// Where the canvas and the window's tab pages go.
    pub(crate) area: HtmlElement,
    pub(crate) canvas: HtmlCanvasElement,
    pub(crate) title_bar: Option<HtmlElement>,
    pub(crate) close: Option<HtmlButtonElement>,
    title: Option<HtmlElement>,
    renderer: WgpuRenderer,
    tiles: HashMap<u32, (AtlasKey, AtlasTile)>,
    pending: Option<(u64, Scene)>,
    device: (u32, u32),
    /// The size the canvas takes with the next frame. Resizing clears it,
    /// so it waits for a frame to draw rather than show a blank window.
    next_device: Option<(u32, u32)>,
    sent: Option<(f32, f32, f32)>,
    /// The size the app lays this window out at: this page's viewport, or
    /// another page's, the one used last.
    layout: Option<(f32, f32, f32)>,
    /// The canvas's CSS size while it shows another page's layout.
    fitted: Option<(f64, f64)>,
    /// CSS pixels per app pixel, for input and placing things on the window.
    pub(crate) scale: f64,
    pub(crate) listeners: Vec<dom::Listener>,
}

/// Whether two viewports are the same layout, give or take rounding.
fn same_layout(a: (f32, f32, f32), b: (f32, f32, f32)) -> bool {
    (a.0 - b.0).abs() < 1. && (a.1 - b.1).abs() < 1. && (a.2 - b.2).abs() < 0.01
}

/// A key no real glyph or image uses, of the kind the tile's atlas needs.
fn key(kind: WireTile, id: u32, width: i32, height: i32) -> AtlasKey {
    match kind {
        WireTile::Mono => AtlasKey::Svg(RenderSvgParams {
            path: format!("herdr-web/{id}").into(),
            size: size(DevicePixels(width), DevicePixels(height)),
        }),
        WireTile::Poly => AtlasKey::Image(RenderImageParams {
            image_id: ImageId(id as usize),
            frame_index: 0,
        }),
        WireTile::Subpixel => AtlasKey::Glyph(RenderGlyphParams {
            font_id: FontId(id as usize),
            glyph_id: GlyphId(0),
            font_size: px(0.),
            subpixel_variant: Point::default(),
            scale_factor: 1.,
            is_emoji: false,
            subpixel_rendering: true,
            dilation: 0,
        }),
    }
}

impl Surface {
    /// Builds the window's elements under `root`. `spare` is the canvas the
    /// GPU was set up on, with its surface, used by the first window.
    pub(crate) fn new(
        id: WindowId,
        main: bool,
        width: f32,
        height: f32,
        context: &WgpuContext,
        spare: Option<(HtmlCanvasElement, wgpu::Surface<'static>)>,
        root: &HtmlElement,
    ) -> Result<Self, String> {
        let element: HtmlElement = dom::create("div").ok_or("no document")?;
        let area: HtmlElement = dom::create("div").ok_or("no document")?;
        area.set_class_name("herdr-canvas-box");
        let (mut title_bar, mut close, mut title) = (None, None, None);
        if main {
            element.set_class_name("herdr-window main");
        } else {
            element.set_class_name("herdr-window floating");
            let bar: HtmlElement = dom::create("div").ok_or("no document")?;
            bar.set_class_name("herdr-titlebar");
            let label: HtmlElement = dom::create("span").ok_or("no document")?;
            let button: HtmlButtonElement = dom::create("button").ok_or("no document")?;
            button.set_text_content(Some("\u{2715}"));
            button.set_title("Close");
            let _ = bar.append_child(&label);
            let _ = bar.append_child(&button);
            let _ = element.append_child(&bar);
            let viewport = dom::window().map_or((1200., 800.), |window| {
                (
                    window
                        .inner_width()
                        .ok()
                        .and_then(|w| w.as_f64())
                        .unwrap_or(1200.),
                    window
                        .inner_height()
                        .ok()
                        .and_then(|h| h.as_f64())
                        .unwrap_or(800.),
                )
            });
            let width = f64::from(width).min(viewport.0 - 24.).max(200.);
            let height = (f64::from(height) + TITLE_HEIGHT)
                .min(viewport.1 - 24.)
                .max(120.);
            dom::style(&element, "width", &format!("{width}px"));
            dom::style(&element, "height", &format!("{height}px"));
            dom::style(
                &element,
                "left",
                &format!("{}px", ((viewport.0 - width) / 2.).max(0.)),
            );
            dom::style(
                &element,
                "top",
                &format!("{}px", ((viewport.1 - height) / 3.).max(0.)),
            );
            title_bar = Some(bar);
            close = Some(button);
            title = Some(label);
        }
        let (canvas, surface) = match spare {
            Some((canvas, surface)) => (canvas, Some(surface)),
            None => (
                dom::create::<HtmlCanvasElement>("canvas").ok_or("no document")?,
                None,
            ),
        };
        canvas.set_tab_index(-1);
        let _ = area.append_child(&canvas);
        let _ = element.append_child(&area);
        let _ = root.append_child(&element);
        let config = WgpuSurfaceConfig {
            size: size(DevicePixels(1), DevicePixels(1)),
            transparent: false,
            preferred_present_mode: None,
        };
        let renderer = match surface {
            Some(surface) => WgpuRenderer::new_from_surface(context, surface, config),
            None => WgpuRenderer::new_from_canvas(context, &canvas, config),
        }
        .map_err(|error| format!("{error:#}"))?;
        Ok(Self {
            id,
            main,
            element,
            area,
            canvas,
            title_bar,
            close,
            title,
            renderer,
            tiles: HashMap::new(),
            pending: None,
            device: (1, 1),
            next_device: None,
            sent: None,
            layout: None,
            fitted: None,
            scale: 1.,
            listeners: Vec::new(),
        })
    }

    pub(crate) fn set_layout(&mut self, layout: (f32, f32, f32)) {
        self.layout = Some(layout);
    }

    pub(crate) fn set_title(&self, text: &str) {
        if let Some(title) = &self.title {
            title.set_text_content(Some(text));
        }
    }

    pub(crate) fn upload(&mut self, tiles: Vec<TileUpload>) {
        let atlas = self.renderer.sprite_atlas().clone();
        for tile in tiles {
            if let Some((old, _)) = self.tiles.remove(&tile.id) {
                gpui::PlatformAtlas::remove(atlas.as_ref(), &old);
            }
            let key = key(tile.kind, tile.id, tile.width, tile.height);
            let tile_size = size(DevicePixels(tile.width), DevicePixels(tile.height));
            let bytes = tile.bytes;
            let placed =
                gpui::PlatformAtlas::get_or_insert_with(atlas.as_ref(), key.clone(), &mut || {
                    Ok(Some((tile_size, Cow::Borrowed(bytes.as_slice()))))
                });
            match placed {
                Ok(Some(placed)) => {
                    self.tiles.insert(tile.id, (key, placed));
                }
                Ok(None) => {}
                Err(error) => dom::log(&format!("Could not place a glyph: {error:#}")),
            }
        }
    }

    pub(crate) fn remove(&mut self, ids: Vec<u32>) {
        let atlas = self.renderer.sprite_atlas().clone();
        for id in ids {
            if let Some((key, _)) = self.tiles.remove(&id) {
                gpui::PlatformAtlas::remove(atlas.as_ref(), &key);
            }
        }
    }

    /// Keeps the newest frame for the next animation frame.
    pub(crate) fn frame(&mut self, seq: u64, scene: WireScene) {
        let scene = scene.into_scene(|_, id| self.tiles.get(&id).map(|(_, tile)| *tile));
        self.pending = Some((seq, scene));
    }

    /// Once per animation frame: follows the canvas's size, then draws the
    /// newest frame. Returns the viewport to report and the frame drawn.
    /// `zoom` is CSS pixels per app pixel: the app lays out in a viewport
    /// `1 / zoom` the canvas's size, at `ratio * zoom` device pixels each.
    pub(crate) fn animate(&mut self, ratio: f64, zoom: f64) -> (Option<ToHost>, Option<u64>) {
        let rect = self.area.get_bounding_client_rect();
        let (width, height) = (rect.width().max(1.), rect.height().max(1.));
        let viewport = (
            (width / zoom) as f32,
            (height / zoom) as f32,
            (ratio * zoom) as f32,
        );
        // Another page's layout: its picture at its own resolution, scaled
        // by the browser to fit this page, top left.
        let (device, fitted, scale) = match self.layout {
            Some(layout) if !same_layout(layout, viewport) => {
                let (app_width, app_height) =
                    (f64::from(layout.0).max(1.), f64::from(layout.1).max(1.));
                let fit = (width / app_width).min(height / app_height);
                (
                    (
                        (app_width * f64::from(layout.2)).round().max(1.) as u32,
                        (app_height * f64::from(layout.2)).round().max(1.) as u32,
                    ),
                    Some((app_width * fit, app_height * fit)),
                    fit,
                )
            }
            _ => (
                (
                    (width * ratio).round().max(1.) as u32,
                    (height * ratio).round().max(1.) as u32,
                ),
                None,
                zoom,
            ),
        };
        self.scale = scale;
        if fitted != self.fitted {
            self.fitted = fitted;
            let (css_width, css_height) = fitted.map_or_else(
                || ("100%".to_owned(), "100%".to_owned()),
                |(w, h)| (format!("{w}px"), format!("{h}px")),
            );
            dom::style(&self.canvas, "width", &css_width);
            dom::style(&self.canvas, "height", &css_height);
        }
        self.next_device = (device != self.device).then_some(device);
        let report = (self.sent != Some(viewport)).then(|| {
            self.sent = Some(viewport);
            ToHost::Viewport {
                window: self.id,
                width: viewport.0,
                height: viewport.1,
                scale: viewport.2,
            }
        });
        let drawn = self.pending.take().map(|(seq, scene)| {
            if let Some(device) = self.next_device.take() {
                self.device = device;
                self.canvas.set_width(device.0);
                self.canvas.set_height(device.1);
                self.renderer.update_drawable_size(size(
                    DevicePixels(device.0 as i32),
                    DevicePixels(device.1 as i32),
                ));
            }
            self.renderer.draw(&scene);
            seq
        });
        (report, drawn)
    }

    /// The window's canvas area on the page, in CSS pixels.
    pub(crate) fn origin(&self) -> (f64, f64) {
        let rect = self.area.get_bounding_client_rect();
        (rect.left(), rect.top())
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.listeners.clear();
        self.renderer.destroy();
        self.element.remove();
    }
}
