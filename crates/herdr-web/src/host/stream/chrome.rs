//! Finding and starting the headless Chrome that browses for streamed pages.
use super::cdp::Cdp;
use std::{
    io::{BufRead as _, BufReader},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, OnceLock},
};

const CANDIDATES: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
];
/// The file that names the Chrome this profile last ran, so a server that
/// was killed does not leave it running.
const PID_FILE: &str = "herdr-chrome.pid";

/// The Chrome to start: `HERDR_WEB_CHROME`, else the first one installed.
pub(super) fn binary() -> Option<&'static Path> {
    static BINARY: OnceLock<Option<PathBuf>> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            std::env::var_os("HERDR_WEB_CHROME")
                .map(PathBuf::from)
                .into_iter()
                .chain(CANDIDATES.iter().map(PathBuf::from))
                .find(|path| path.is_file())
        })
        .as_deref()
}

/// Stops the Chrome a previous server left on `profile`, if that process is
/// still the one it started: its command line names the profile.
fn stop_leftover(profile: &Path) {
    let Ok(pid) = std::fs::read_to_string(profile.join(PID_FILE)) else {
        return;
    };
    let pid = pid.trim();
    if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return;
    }
    let Ok(output) = herdr_client::new_command("ps")
        .args(["-p", pid, "-o", "command="])
        .output()
    else {
        return;
    };
    let command = String::from_utf8_lossy(&output.stdout);
    if command.contains(&format!("--user-data-dir={}", profile.display())) {
        let _ = herdr_client::new_command("kill").arg(pid).status();
    }
}

/// Starts a headless Chrome on `profile` and connects to it.
pub(super) fn launch(profile: &Path) -> crate::Result<Arc<Cdp>> {
    let binary = binary().ok_or(crate::Error::Unsupported)?;
    std::fs::create_dir_all(profile)?;
    stop_leftover(profile);
    let mut child = herdr_client::new_command(binary)
        .args([
            "--headless=new",
            "--remote-debugging-address=127.0.0.1",
            "--remote-debugging-port=0",
            "--no-first-run",
            "--no-default-browser-check",
            "--mute-audio",
            "--hide-scrollbars",
            // The screencast draws at the browser's own density, not the
            // page's emulated one: twice is sharp on phones and Retina.
            "--force-device-scale-factor=2",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let _ = std::fs::write(profile.join(PID_FILE), child.id().to_string());
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        return Err(crate::Error::Unsupported);
    };
    let mut lines = BufReader::new(stderr).lines();
    let url = loop {
        match lines.next() {
            Some(Ok(line)) => {
                if let Some(url) = line.strip_prefix("DevTools listening on ") {
                    break url.trim().to_owned();
                }
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(crate::Error::Unsupported);
            }
        }
    };
    // Chrome keeps writing; an unread pipe would stall it.
    let _ = std::thread::Builder::new()
        .name("web-chrome-log".into())
        .spawn(move || for _ in lines {});
    match Cdp::connect(&url) {
        Ok(cdp) => {
            cdp.adopt(child);
            tracing::info!("Started Chrome for browser tabs");
            Ok(cdp)
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}
