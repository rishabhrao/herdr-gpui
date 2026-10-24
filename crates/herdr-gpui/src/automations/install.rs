//! Enabling automations on a host that has no `herdr-automate`, or
//! updating one that runs another version than this app ships: copy the
//! build for the host's platform into the user's home, link it onto
//! `PATH`, and start the daemon. Nothing needs root and nothing is written
//! outside `$HOME`:
//!
//! - `~/.local/share/herdr-automate/bin/VERSION/herdr-automate`, the build;
//! - `~/.local/bin/herdr-automate`, a link to it;
//! - a user service from `herdr-automate install-service` (a systemd user
//!   unit or a launchd agent), when enabling.
//!
//! The bytes go through the host script's stdin and are checked against
//! their SHA-256 on the host before they are moved into place.

use super::{Error, fetch};
use crate::teleport::Host;
use herdr_client::{ScriptLimits, shell_quote};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

/// Where builds for other platforms ship, beside the app's executable.
const BUILDS: &str = "automate";
/// A slow link can take a while to carry a few megabytes.
const COPY_IDLE: Duration = Duration::from_secs(300);
/// What the install script itself may print.
const COPY_OUTPUT: u64 = 64 * 1024;
/// `shutdown` waits up to about 40 s for runs to finish before it answers.
const SHUTDOWN_IDLE: Duration = Duration::from_secs(90);
/// The install script's exit status when the copy did not match.
const CHECKSUM_EXIT: i32 = 3;

/// A platform as `uname -sm` names it, in Rust's words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Platform {
    /// `linux` or `macos`.
    pub(crate) os: &'static str,
    /// `x86_64` or `aarch64`.
    pub(crate) arch: &'static str,
}

impl Platform {
    /// Reads `uname -sm` output such as `Linux x86_64` or `Darwin arm64`.
    pub(crate) fn parse(uname: &str) -> Result<Self, Error> {
        let mut words = uname.split_whitespace();
        let (Some(system), Some(machine)) = (words.next(), words.next()) else {
            return Err(Error::Platform(crate::notifications::safe_text(uname, 80)));
        };
        let os = match system {
            "Linux" => "linux",
            "Darwin" => "macos",
            _ => return Err(Error::Platform(crate::notifications::safe_text(uname, 80))),
        };
        let arch = match machine {
            "x86_64" | "amd64" => "x86_64",
            "aarch64" | "arm64" => "aarch64",
            _ => return Err(Error::Platform(crate::notifications::safe_text(uname, 80))),
        };
        Ok(Self { os, arch })
    }

    /// This computer's platform.
    pub(crate) fn local() -> Self {
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }

    /// The target a build for this platform is made for.
    pub(crate) fn triple(&self) -> String {
        match self.os {
            "macos" => format!("{}-apple-darwin", self.arch),
            _ => format!("{}-unknown-linux-musl", self.arch),
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let os = match self.os {
            "macos" => "macOS",
            "linux" => "Linux",
            other => other,
        };
        write!(f, "{os} {}", self.arch)
    }
}

/// What enabling copies, shown before the user confirms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) platform: Platform,
    pub(crate) source: PathBuf,
    pub(crate) version: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}

/// The build for `platform` this app ships: its own `herdr-automate` when
/// the host is this kind of computer, else `automate/TRIPLE/herdr-automate`
/// beside the executable.
pub(crate) fn build_for(
    platform: &Platform,
    beside: &Path,
    bundled: Option<PathBuf>,
) -> Option<PathBuf> {
    let same = *platform == Platform::local();
    if let Some(bundled) = bundled.filter(|_| same) {
        return Some(bundled);
    }
    let path = beside
        .join(BUILDS)
        .join(platform.triple())
        .join(fetch::PROGRAM);
    path.is_file().then_some(path)
}

/// Finds out what enabling `host` would copy. Blocks.
pub(crate) fn prepare(
    host: &Host,
    shipped: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Plan, Error> {
    // Builds for every platform ship together, so each has the version the
    // app's own `herdr-automate --version` printed; it names the directory.
    let version = shipped.ok_or(Error::UnknownVersion)?.to_owned();
    let uname = host.capture("uname -sm\n", cancelled).map_err(Error::Run)?;
    let platform = Platform::parse(&String::from_utf8_lossy(&uname))?;
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let source = build_for(&platform, &beside, fetch::bundled()).ok_or_else(|| Error::NoBuild {
        platform: platform.to_string(),
    })?;
    let (size, sha256) = digest(&source)?;
    Ok(Plan {
        platform,
        source,
        version,
        size,
        sha256,
    })
}

/// The file's size and SHA-256, read in pieces.
pub(crate) fn digest(path: &Path) -> Result<(u64, String), Error> {
    let binary = |source| Error::Binary {
        path: path.to_owned(),
        source,
    };
    let mut file = std::fs::File::open(path).map_err(binary)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer).map_err(binary)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    let sha256 = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((size, sha256))
}

/// The host script that receives the build on stdin and puts it in place.
pub(crate) fn script(plan: &Plan) -> String {
    format!(
        r#"version={version}
sum={sum}
dir="$HOME/.local/share/herdr-automate/bin/$version"
mkdir -p "$dir" "$HOME/.local/bin"
part="$dir/.herdr-automate.part.$$"
trap 'rm -f "$part"' EXIT
cat > "$part"
if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$part"); else got=$(shasum -a 256 "$part"); fi
got=${{got%% *}}
if [ "$got" != "$sum" ]; then echo "the copy does not match its checksum" >&2; exit {CHECKSUM_EXIT}; fi
chmod 755 "$part"
mv -f "$part" "$dir/herdr-automate"
ln -sfn "$dir/herdr-automate" "$HOME/.local/bin/herdr-automate"
"#,
        version = shell_quote(&plan.version),
        sum = shell_quote(&plan.sha256),
    )
}

/// Copies the build and starts the daemon; what the host said. Blocks.
pub(crate) fn install(
    host: &Host,
    plan: &Plan,
    update: bool,
    cancelled: &AtomicBool,
) -> Result<String, Error> {
    let file = std::fs::File::open(&plan.source).map_err(|source| Error::Binary {
        path: plan.source.clone(),
        source,
    })?;
    let mut output = Vec::new();
    host.stream(
        &script(plan),
        file,
        &mut output,
        ScriptLimits {
            output: COPY_OUTPUT,
            idle: COPY_IDLE,
        },
        cancelled,
    )
    .map_err(|error| match &error {
        herdr_client::Error::ScriptExit { status, .. } if status.code() == Some(CHECKSUM_EXIT) => {
            Error::Checksum
        }
        _ => Error::Run(error),
    })?;
    if update {
        // `start` does nothing while a daemon holds the lock: stop the old
        // one first. It lets its runs finish, which can take a while.
        fetch::act_within(host, &["shutdown"], SHUTDOWN_IDLE, cancelled)?;
    }
    // On an update too: the service names the executable it runs, and the
    // one written before names the old version.
    fetch::act(host, &["install-service"], cancelled)?;
    let started = fetch::act(host, &["start"], cancelled)?;
    Ok(started.message)
}
