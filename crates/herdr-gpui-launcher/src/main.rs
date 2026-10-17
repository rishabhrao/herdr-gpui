#![cfg_attr(windows, windows_subsystem = "windows")]
//! `herdr-gpui-launcher.exe`: starts `herdr-gpui.exe` from the same directory without a
//! console window. `herdr-gpui.exe` stays a console program because it is also
//! the CLI, so Explorer and the Start menu open this launcher instead.

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    match windows::launch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            windows::show_error(&windows::message(&error));
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!(
        "herdr-gpui-launcher.exe is the Windows launcher for herdr-gpui; run herdr-gpui instead."
    );
    ExitCode::from(2)
}

#[cfg(windows)]
mod windows {
    use std::{
        error::Error as _,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };

    use herdr_gpui_launcher::{Error, Result, gui_path};

    /// Gives the GUI a console that has no window, rather than no console at
    /// all (`DETACHED_PROCESS`): console programs the GUI starts later (git,
    /// gh, wsl.exe) inherit that hidden console instead of each opening one.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const TITLE: &str = "Herdr UI";

    /// Starts the GUI with this launcher's arguments and returns once it has
    /// started; the GUI is never waited on and keeps running after we exit.
    pub fn launch() -> Result<()> {
        let launcher = std::env::current_exe().map_err(Error::CurrentExe)?;
        let gui = gui_path(&launcher)?;
        Command::new(&gui)
            .args(std::env::args_os().skip(1))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|source| Error::Spawn { gui, source })?;
        Ok(())
    }

    /// The error and each of its causes, for the message box.
    pub fn message(error: &Error) -> String {
        let mut text = error.to_string();
        let mut source = error.source();
        while let Some(cause) = source {
            text.push_str(": ");
            text.push_str(&cause.to_string());
            source = cause.source();
        }
        text
    }

    /// Shows `text` in an error message box. A windowed program has no console
    /// to print to, and the standard library has no message box, so this calls
    /// `MessageBoxW` directly.
    #[allow(unsafe_code)]
    pub fn show_error(text: &str) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let text = wide(text);
        let title = wide(TITLE);
        // SAFETY: the owner window is null, which `MessageBoxW` documents as
        // "no owner". `text` and `title` are NUL-terminated UTF-16 buffers that
        // live until the end of this function, past the call, and the call
        // writes through no pointer.
        let shown = unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            )
        };
        // Zero means the box could not be shown; with no console and no window
        // there is nowhere left to report that, and the exit code still says 1.
        let _ = shown;
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}
