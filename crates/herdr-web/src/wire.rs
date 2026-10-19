//! What the app and the page send each other. Both ends are built from the
//! same commit, so the format carries a protocol number rather than
//! negotiating; a page from another build is told to reload.
mod scene;

pub use scene::{WireScene, WireTile};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Bumped whenever a message changes shape.
pub const PROTOCOL: u32 = 9;
/// No message the page sends is larger; the host refuses bigger frames.
pub const MAX_TO_HOST_BYTES: usize = 16 * 1024 * 1024;
/// A frame of a very dense window, with room to spare; larger ones are refused.
pub const MAX_TO_BROWSER_BYTES: usize = 64 * 1024 * 1024;

/// A GPUI window's id, the same on both ends.
pub type WindowId = u64;
/// A browser tab's page, numbered by the app.
pub type PageId = u64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToBrowser {
    Hello {
        protocol: u32,
    },
    /// The size window `window` lays out at now, in app pixels, and its
    /// pixel ratio: the viewport of the page used last. A page whose own
    /// viewport differs shows the window scaled to fit.
    Layout {
        window: WindowId,
        width: f32,
        height: f32,
        scale: f32,
    },
    /// A link the app opened, for the page to offer: a remote phone cannot
    /// see the Mac's browser.
    OpenUrl {
        url: String,
    },
    /// Text the app copied, for the page's own clipboard.
    Clipboard {
        text: String,
    },
    /// A notification the app posted.
    Notify {
        title: String,
        body: String,
    },
    /// A short note about something this page cannot do.
    Toast {
        text: String,
    },
    /// The app's keys a touch keys bar offers: Herdr's prefix, as configured.
    Keys {
        prefix: Option<Chord>,
    },
    /// Something to do in the browser that asked from Settings, such as
    /// sizing its text: only it, not every device showing the app.
    Device(DeviceAction),
    /// A question the app asks, as a dialog of the page's own: `level` is
    /// 0 for information, 1 for a warning, 2 for something critical.
    Prompt {
        id: u64,
        level: u8,
        message: String,
        detail: Option<String>,
        answers: Vec<String>,
    },
    /// The question was answered elsewhere, or no longer matters.
    PromptClosed {
        id: u64,
    },
    /// A window to show, at `width` by `height` CSS pixels. The first main
    /// window fills the page; the others float above it.
    WindowOpened {
        window: WindowId,
        title: String,
        main: bool,
        width: f32,
        height: f32,
    },
    WindowClosed {
        window: WindowId,
    },
    Title {
        window: WindowId,
        title: String,
    },
    /// Brings a floating window to the front.
    Activate {
        window: WindowId,
    },
    /// Glyphs and images the following frames draw.
    Upload {
        window: WindowId,
        tiles: Vec<TileUpload>,
    },
    Remove {
        window: WindowId,
        tiles: Vec<u32>,
    },
    Frame {
        window: WindowId,
        seq: u64,
        scene: WireScene,
    },
    /// A CSS `cursor` value.
    Cursor {
        css: String,
    },
    /// Where text input happens, so the page can place the IME there.
    Ime {
        window: WindowId,
        bounds: [f32; 4],
    },
    Page(PageOp),
}

/// What a browser tab's page should do. Pages are `<iframe>`s laid over the
/// window at the bounds the app gives, in CSS pixels.
/// A glyph or image `width` by `height` device pixels: one byte per pixel
/// for [`WireTile::Mono`], four (BGRA) for [`WireTile::Poly`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileUpload {
    pub id: u32,
    pub kind: WireTile,
    pub width: i32,
    pub height: i32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PageOp {
    Open {
        window: WindowId,
        page: PageId,
        url: String,
        /// A local file the app serves; it may post messages to the app.
        local: bool,
    },
    Place {
        page: PageId,
        bounds: [f32; 4],
    },
    Hide {
        page: PageId,
    },
    Close {
        page: PageId,
    },
    Load {
        page: PageId,
        url: String,
    },
    /// Runs one of the app's own scripts in a local page; web pages are on
    /// another origin and cannot be scripted.
    Script {
        page: PageId,
        script: String,
    },
    Back {
        page: PageId,
    },
    Forward {
        page: PageId,
    },
    Reload {
        page: PageId,
    },
    Focus {
        page: PageId,
    },
    Blur {
        page: PageId,
    },
    /// A web page browsed on the host: the page shows its pictures, which
    /// [`PageOp::Frame`] sends, in place of an `<iframe>`, and sends its
    /// input back as [`PageEvent::Input`].
    Stream {
        window: WindowId,
        page: PageId,
    },
    /// The streamed page's latest picture, a JPEG.
    Frame {
        page: PageId,
        jpeg: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToHost {
    /// The page's first message. Every device's page shows the app at once.
    Hello {
        protocol: u32,
    },
    /// The page's clipboard text, before a paste.
    Clipboard {
        text: String,
    },
    /// An image the page pastes, such as one a phone keyboard inserts.
    ClipboardImage {
        mime: String,
        bytes: Vec<u8>,
    },
    /// The index of the answer chosen in prompt `id`.
    PromptAnswer {
        id: u64,
        answer: u32,
    },
    /// The window's canvas size in CSS pixels and the page's device pixel ratio.
    Viewport {
        window: WindowId,
        width: f32,
        height: f32,
        scale: f32,
    },
    /// The page is ready for the next frame of `window`.
    Tick {
        window: WindowId,
    },
    /// The page drew frame `seq`.
    Ack {
        window: WindowId,
        seq: u64,
    },
    Input {
        window: WindowId,
        input: Input,
    },
    Text {
        window: WindowId,
        op: TextOp,
    },
    Focus {
        window: WindowId,
        focused: bool,
    },
    Visible {
        visible: bool,
    },
    Hover {
        window: WindowId,
        hovered: bool,
    },
    /// The page's close button on a floating window.
    Close {
        window: WindowId,
    },
    Dark {
        dark: bool,
    },
    Page(PageEvent),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PageEvent {
    Loaded {
        page: PageId,
        url: String,
    },
    Title {
        page: PageId,
        title: String,
    },
    /// A local page's `herdr.send`; untrusted text.
    Posted {
        page: PageId,
        body: String,
    },
    /// Input for a streamed page, in its CSS pixels.
    Input {
        page: PageId,
        input: StreamInput,
    },
}

/// A key with its modifiers, named as GPUI names keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chord {
    pub key: String,
    pub mods: Mods,
}

/// What Settings > Web access asks of the browser it is used in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceAction {
    TextSmaller,
    TextReset,
    TextLarger,
    AllowNotifications,
    SignOut,
    /// Puts the phone keyboard down: the app covered what it typed into.
    HideKeyboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseKind {
    Down,
    Up,
    Move,
}

/// What a streamed page is sent: pointer and wheel in its CSS pixels, keys
/// by their DOM `key` and `code`, and text from a keyboard or IME.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StreamInput {
    Mouse {
        kind: MouseKind,
        x: f32,
        y: f32,
        button: Option<Button>,
        clicks: u8,
        mods: Mods,
    },
    Wheel {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    },
    Key {
        down: bool,
        key: String,
        code: String,
        /// The text a press types, if any.
        text: Option<String>,
        mods: Mods,
    },
    Text(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mods {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub platform: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Button {
    Left,
    Middle,
    Right,
    Back,
    Forward,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Delta {
    Pixels(f32, f32),
    Lines(f32, f32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Input {
    KeyDown {
        key: String,
        key_char: Option<String>,
        mods: Mods,
        held: bool,
    },
    KeyUp {
        key: String,
        key_char: Option<String>,
        mods: Mods,
    },
    Modifiers {
        mods: Mods,
        capslock: bool,
    },
    MouseDown {
        button: Button,
        x: f32,
        y: f32,
        mods: Mods,
        clicks: u8,
    },
    MouseUp {
        button: Button,
        x: f32,
        y: f32,
        mods: Mods,
        clicks: u8,
    },
    MouseMove {
        x: f32,
        y: f32,
        pressed: Option<Button>,
        mods: Mods,
    },
    MouseExit {
        x: f32,
        y: f32,
        pressed: Option<Button>,
        mods: Mods,
    },
    Scroll {
        x: f32,
        y: f32,
        delta: Delta,
        mods: Mods,
    },
    Pinch {
        x: f32,
        y: f32,
        delta: f32,
        mods: Mods,
    },
}

/// Text the page's hidden input produced outside of a plain key press.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TextOp {
    /// Dictation, the emoji picker, or a key the IME let through as text.
    Insert(String),
    /// The IME's text so far, and the selection inside it in UTF-16 units.
    Compose {
        text: String,
        selected: Option<(u32, u32)>,
    },
    Commit(String),
    Cancel,
}

/// The limit also bounds what a decoder allocates for a claimed length.
fn config() -> impl bincode::config::Config {
    bincode::config::standard().with_limit::<MAX_TO_BROWSER_BYTES>()
}

pub fn encode(message: &impl Serialize) -> crate::Result<Vec<u8>> {
    Ok(bincode::serde::encode_to_vec(message, config())?)
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> crate::Result<T> {
    let (message, read) = bincode::serde::decode_from_slice(bytes, config())?;
    match bytes.len() - read {
        0 => Ok(message),
        trailing => Err(crate::Error::Trailing(trailing)),
    }
}

#[cfg(test)]
mod tests;
