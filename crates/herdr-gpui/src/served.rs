//! `herdr-gpui serve`: the whole app, with its windows drawn in a browser tab
//! instead of on the desktop, on this machine or through a tunnel. The app
//! itself runs as usual; `herdr_web` supplies the platform and the server.
use crate::cli::{ServeAction, ServeOptions};
use gpui::App;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
use gpui::{AsyncApp, PromptLevel};
use std::sync::atomic::{AtomicBool, Ordering};

/// Pairing requests to ask about, from the server.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
pub(crate) use herdr_web::host::Approvals;

/// No server runs on this platform, so no request ever comes.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
pub(crate) enum Approvals {}

static SERVED: AtomicBool = AtomicBool::new(false);

/// Whether this process draws its windows in a browser tab. Set once,
/// before the first window opens.
pub(crate) fn active() -> bool {
    SERVED.load(Ordering::Relaxed)
}

/// Why `serve` would not start, in words for the terminal.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ServeError {
    #[error(transparent)]
    Web(#[from] herdr_web::Error),
    #[error("--access-team and --access-aud go together")]
    HalfAccess,
    #[error("the state folder is unknown: set XDG_STATE_HOME or HOME")]
    NoStateDir,
}

/// Values from the environment when the flags leave them out, so a tunnel
/// setup can live in the shell profile.
fn from_env(serve: &ServeOptions) -> ServeOptions {
    let mut serve = serve.clone();
    let var = |name| {
        std::env::var(name)
            .ok()
            .filter(|value: &String| !value.is_empty())
    };
    if serve.public_hosts.is_empty()
        && let Some(hosts) = var("HERDR_WEB_PUBLIC_HOSTS")
    {
        serve.public_hosts = hosts.split(',').map(str::trim).map(String::from).collect();
    }
    serve.access_team = serve.access_team.or_else(|| var("HERDR_WEB_ACCESS_TEAM"));
    serve.access_audience = serve
        .access_audience
        .or_else(|| var("HERDR_WEB_ACCESS_AUD"));
    serve
}

/// The Access application named up front: its team and audience.
fn access(serve: &ServeOptions) -> Result<Option<(String, String)>, ServeError> {
    match (&serve.access_team, &serve.access_audience) {
        (Some(team), Some(audience)) => Ok(Some((team.clone(), audience.clone()))),
        // Cloudflare Access in front of the tunnel is the lock; checking its
        // token here as well is optional.
        (None, None) => Ok(None),
        _ => Err(ServeError::HalfAccess),
    }
}

/// The application, native or served, and for a served one the pairing
/// requests to ask about. The server binds here, so a taken port or an
/// unbuilt page fails before any window opens.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
pub(crate) fn application(
    serve: Option<&ServeOptions>,
) -> Result<(gpui::Application, Option<Approvals>), ServeError> {
    let Some(serve) = serve else {
        return Ok((gpui_platform::application(), None));
    };
    let serve = from_env(serve);
    let access =
        access(&serve)?.map(|(team, audience)| herdr_web::host::AccessOptions { team, audience });
    let state_dir = crate::preferences::state_dir().ok_or(ServeError::NoStateDir)?;
    let server = herdr_web::host::bind(&herdr_web::host::Options {
        port: serve.port,
        client_dir: herdr_web::host::default_client_dir(),
        state_dir,
        public_hosts: serve.public_hosts.clone(),
        access: access.clone(),
        check_access: !serve.no_access_check,
    })?;
    SERVED.store(true, Ordering::Relaxed);
    tracing::info!(port = serve.port, "Serving Herdr UI to a browser");
    println!(
        "Herdr UI is served at {}\nOpen it in a browser on this Mac. Press Ctrl+C to stop.",
        server.url()
    );
    for host in server.public_hosts() {
        println!(
            "Also at https://{host}: new devices ask for access, and you approve them in Herdr UI."
        );
    }
    if !serve.public_hosts.is_empty() && !serve.no_access_check {
        println!(
            "Requests through the public host need a valid Cloudflare Access token; the first one names the Access application."
        );
    }
    let approvals = server.approvals();
    // Headless: no Dock icon, menu bar, or native windows.
    let native = gpui_platform::current_platform(true);
    Ok((
        gpui::Application::with_platform(server.into_platform(native)),
        Some(approvals),
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
pub(crate) fn application(
    serve: Option<&ServeOptions>,
) -> Result<(gpui::Application, Option<Approvals>), ServeError> {
    let Some(serve) = serve else {
        return Ok((gpui_platform::application(), None));
    };
    // Wrong flags are still named before the platform is.
    access(&from_env(serve))?;
    Err(herdr_web::Error::Unsupported.into())
}

/// `serve devices` and `serve forget NAME`, which need no running app.
/// `None` for `serve` itself, which starts it.
pub(crate) fn manage(serve: Option<&ServeOptions>) -> Option<std::process::ExitCode> {
    let action = &serve?.action;
    if *action == ServeAction::Run {
        return None;
    }
    let Some(state_dir) = crate::preferences::state_dir() else {
        eprintln!("{}", ServeError::NoStateDir);
        return Some(std::process::ExitCode::FAILURE);
    };
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
    match action {
        ServeAction::Run => None,
        ServeAction::Devices => {
            let devices = herdr_web::host::list_devices(&state_dir);
            if devices.is_empty() {
                println!("No device is paired.");
            }
            for device in devices {
                let place = if device.remote { "remote" } else { "this Mac" };
                println!("{}  {}  ({place})", device.id, device.label);
            }
            Some(std::process::ExitCode::SUCCESS)
        }
        ServeAction::Forget(name) if name.is_empty() => {
            eprintln!("serve forget requires a device's name or id (see serve devices)");
            Some(std::process::ExitCode::from(2))
        }
        ServeAction::Forget(name) => match herdr_web::host::forget_device(&state_dir, name) {
            Ok(0) => {
                eprintln!("No paired device is called {name}");
                Some(std::process::ExitCode::FAILURE)
            }
            Ok(count) => {
                println!("Forgot {count} device(s). Their next request is refused.");
                Some(std::process::ExitCode::SUCCESS)
            }
            Err(error) => {
                eprintln!("Could not forget {name}: {error}");
                Some(std::process::ExitCode::FAILURE)
            }
        },
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
    {
        let _ = state_dir;
        eprintln!("{}", herdr_web::Error::Unsupported);
        Some(std::process::ExitCode::FAILURE)
    }
}

/// Asks whoever uses the app about each browser that wants to pair. The
/// dialog shows in the app's own window, so it reaches this Mac's browser or
/// any device already paired.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
pub(crate) fn ask_about_devices(approvals: Approvals, cx: &mut App) {
    answer_from_terminal(approvals.clone());
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some(request) = approvals.next().await {
            // Answered elsewhere already, in the terminal.
            let Some(allow) = ask(&request, &approvals, cx).await else {
                continue;
            };
            match approvals.answer(request.id, allow) {
                Ok(true) => println!(
                    "{} {}.",
                    request.label,
                    if allow {
                        "may now use Herdr UI"
                    } else {
                        "was denied"
                    }
                ),
                Ok(false) => println!("{}'s request ran out of time.", request.label),
                Err(error) => eprintln!("Could not answer {}'s request: {error}", request.label),
            }
        }
    })
    .detach();
}

/// `allow CODE` or `deny CODE` typed where `serve` runs, so a device can be
/// let in with no paired screen open. Reads until the terminal closes.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
fn answer_from_terminal(approvals: Approvals) {
    let spawned = std::thread::Builder::new()
        .name("serve-approvals".into())
        .spawn(move || {
            for line in std::io::stdin().lines() {
                let Ok(line) = line else {
                    return;
                };
                let mut words = line.split_whitespace();
                let (allow, code) = match (words.next(), words.next()) {
                    (Some("allow"), Some(code)) => (true, code),
                    (Some("deny"), Some(code)) => (false, code),
                    (None, _) => continue,
                    _ => {
                        println!("Type allow CODE or deny CODE.");
                        continue;
                    }
                };
                match approvals.answer_code(code, allow) {
                    Ok(true) => println!("{} {code}.", if allow { "Allowed" } else { "Denied" }),
                    Ok(false) => println!("No device waits with code {code}."),
                    Err(error) => println!("Could not answer {code}: {error}"),
                }
            }
        });
    if spawned.is_err() {
        eprintln!("Approving devices from this terminal is not available.");
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
async fn ask(
    request: &herdr_web::host::PairingRequest,
    approvals: &Approvals,
    cx: &mut AsyncApp,
) -> Option<bool> {
    let detail = format!(
        "{} asks to use Herdr UI with code {}.\n\nAllow it only if that device shows the same code. An allowed device can type in every terminal.",
        request.label, request.code
    );
    let window = cx.update(|cx| cx.windows().into_iter().next())?;
    let Ok(mut answer) = window.update(cx, |_, window, cx| {
        window.prompt(
            PromptLevel::Warning,
            "Allow a new device?",
            Some(&detail),
            &["Allow", "Deny"],
            cx,
        )
    }) else {
        return None;
    };
    // Answered in the terminal, or out of time: dropping the receiver
    // closes the dialog on the page.
    loop {
        match answer.try_recv() {
            Ok(Some(choice)) => return Some(choice == 0),
            Ok(None) => {}
            Err(_) => return None,
        }
        if !approvals.is_waiting(request.id) {
            return None;
        }
        cx.background_executor()
            .timer(std::time::Duration::from_millis(300))
            .await;
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
pub(crate) fn ask_about_devices(approvals: Approvals, _: &mut App) {
    match approvals {}
}
