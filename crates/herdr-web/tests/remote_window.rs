//! Opens a real GPUI window on the remote platform, connects to it the way
//! the page does, and checks the frame it sends: text arrives as glyph tiles
//! the frame's sprites name. It guards GPUI upgrades, which can change the
//! platform traits and scene types this crate relies on.
//!
//! macOS creates its platform on the main thread, so this test owns `main`.

#[cfg(target_os = "macos")]
fn main() {
    macos::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("remote_window: skipped, needs macOS");
}

#[cfg(target_os = "macos")]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod macos {
    use gpui::{
        AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _, Window,
        WindowOptions, div, px, rgb,
    };
    use herdr_web::{
        host,
        wire::{self, PROTOCOL, ToBrowser, ToHost},
    };
    use std::{
        collections::HashSet,
        io::{BufRead as _, BufReader, Read, Write},
        net::TcpStream,
        time::{Duration, Instant},
    };

    struct Hello;

    impl Render for Hello {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(rgb(0x202020))
                .text_color(rgb(0xffffff))
                .text_size(px(20.))
                .child("Hello from the remote platform")
        }
    }

    fn fail(message: &str) -> ! {
        eprintln!("remote_window: FAILED: {message}");
        std::process::exit(1);
    }

    pub(super) fn run() {
        let dir = tempfile_dir();
        std::fs::write(dir.join("herdr_web_client.js"), b"export default () => {}").unwrap();
        std::fs::write(dir.join("herdr_web_client_bg.wasm"), b"\0asm").unwrap();
        let server = host::bind(&host::Options {
            port: 0,
            client_dir: dir.clone(),
            state_dir: dir.clone(),
            public_hosts: Vec::new(),
            access: None,
            check_access: true,
        })
        .unwrap();
        let url = server.url();
        let platform = server.into_platform(gpui_platform::current_platform(true));
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| page(&url));
            let _ = std::fs::remove_dir_all(&dir);
            match result {
                Ok(()) => {
                    println!("remote_window: ok");
                    std::process::exit(0);
                }
                Err(_) => fail("the page side panicked"),
            }
        });
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(60));
            fail("timed out");
        });
        gpui::Application::with_platform(platform).run(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Hello))
                .expect("window opens on the remote platform");
        });
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("herdr-web-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn masked(payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![0x82];
        match payload.len() {
            length @ 0..=125 => frame.push(0x80 | length as u8),
            length => {
                frame.push(0x80 | 126);
                frame.extend_from_slice(&(length as u16).to_be_bytes());
            }
        }
        frame.extend_from_slice(&[0, 0, 0, 0]);
        frame.extend_from_slice(payload);
        frame
    }

    fn send(stream: &mut TcpStream, message: &ToHost) {
        stream
            .write_all(&masked(&wire::encode(message).unwrap()))
            .unwrap();
    }

    fn receive(stream: &mut impl Read) -> ToBrowser {
        let mut head = [0; 2];
        stream.read_exact(&mut head).unwrap();
        let length = match head[1] & 0x7F {
            126 => {
                let mut bytes = [0; 2];
                stream.read_exact(&mut bytes).unwrap();
                u64::from(u16::from_be_bytes(bytes))
            }
            127 => {
                let mut bytes = [0; 8];
                stream.read_exact(&mut bytes).unwrap();
                u64::from_be_bytes(bytes)
            }
            short => u64::from(short),
        };
        let mut payload = vec![0; length as usize];
        stream.read_exact(&mut payload).unwrap();
        wire::decode(&payload).unwrap()
    }

    /// Opens the token address as this machine's browser does: the reply
    /// pairs it and sets the device cookie.
    fn pair(port: &str, query: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
        write!(
            stream,
            "GET /{} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n",
            query.trim_start_matches('/')
        )
        .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 302"), "pairing: {reply}");
        reply
            .lines()
            .find_map(|line| line.strip_prefix(&format!("Set-Cookie: herdr_device_{port}=")))
            .and_then(|rest| rest.split(';').next())
            .unwrap()
            .to_owned()
    }

    fn page(url: &str) {
        let rest = url.strip_prefix("http://localhost:").unwrap();
        let (port, query) = rest.split_once('/').unwrap();
        let device = pair(port, query);
        let mut stream = TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        write!(
            stream,
            "GET /ws HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nCookie: herdr_device_{port}={device}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )
        .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut status = String::new();
        reader.read_line(&mut status).unwrap();
        assert!(status.starts_with("HTTP/1.1 101"), "handshake: {status}");
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        send(&mut stream, &ToHost::Hello { protocol: PROTOCOL });

        let mut window = None;
        let mut tiles = HashSet::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match receive(&mut reader) {
                ToBrowser::WindowOpened {
                    window: id, main, ..
                } => {
                    assert!(main, "the first normal window fills the page");
                    window = Some(id);
                    send(
                        &mut stream,
                        &ToHost::Viewport {
                            window: id,
                            width: 640.,
                            height: 480.,
                            scale: 2.,
                        },
                    );
                    send(&mut stream, &ToHost::Tick { window: id });
                }
                ToBrowser::Upload {
                    tiles: uploaded, ..
                } => {
                    for tile in uploaded {
                        assert_eq!(
                            tile.bytes.len(),
                            (tile.width * tile.height) as usize
                                * if tile.kind == wire::WireTile::Mono {
                                    1
                                } else {
                                    4
                                }
                        );
                        tiles.insert(tile.id);
                    }
                }
                ToBrowser::Frame {
                    window: id,
                    seq,
                    scene,
                } => {
                    assert_eq!(Some(id), window);
                    send(&mut stream, &ToHost::Ack { window: id, seq });
                    let mut named = HashSet::new();
                    let scene = scene.into_scene(|_, tile| {
                        named.insert(tile);
                        None
                    });
                    if !named.is_empty() {
                        assert!(
                            named.is_subset(&tiles),
                            "every sprite's tile was uploaded first"
                        );
                        assert!(!scene.quads.is_empty(), "the background is a quad");
                        return;
                    }
                    send(&mut stream, &ToHost::Tick { window: id });
                }
                _ => {}
            }
        }
        panic!("no frame with text arrived");
    }
}
