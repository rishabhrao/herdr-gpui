//! The page `herdr-gpui serve` shows. It draws each window's frames with
//! GPUI's own wgpu renderer and sends input back; the app itself keeps
//! running on the machine that serves it.
#[cfg(target_family = "wasm")]
mod app;
#[cfg(target_family = "wasm")]
mod dom;
#[cfg(target_family = "wasm")]
mod exports;
#[cfg(target_family = "wasm")]
mod input;
#[cfg(target_family = "wasm")]
mod keys;
#[cfg(target_family = "wasm")]
mod pages;
#[cfg(target_family = "wasm")]
mod surface;
#[cfg(target_family = "wasm")]
mod touch;

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    wasm_bindgen_futures::spawn_local(async {
        if let Err(error) = app::run().await {
            dom::show_status(&format!("Herdr UI could not start: {error}"));
        }
    });
}
