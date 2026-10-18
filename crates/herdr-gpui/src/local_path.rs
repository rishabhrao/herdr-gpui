use std::{ffi::OsString, path::PathBuf};

/// GUI launches omit directories commonly used by user-installed tools.
pub(crate) fn local_path() -> OsString {
    let mut dirs = Vec::new();
    if let Ok(home) = crate::config::home() {
        dirs.extend(
            [".local/bin", ".cargo/bin", ".bun/bin", ".npm-global/bin"].map(|dir| home.join(dir)),
        );
    }
    if cfg!(unix) {
        dirs.extend(
            [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/home/linuxbrew/.linuxbrew/bin",
            ]
            .map(PathBuf::from),
        );
    }
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    dirs.extend(std::env::split_paths(&inherited));
    // launchd's `PATH` stops at `/usr/bin:/bin`; system tools such as `lsof`,
    // which the Tailscale CLI runs on macOS, live in the `sbin` directories.
    if cfg!(unix) {
        for system in ["/usr/sbin", "/sbin"].map(PathBuf::from) {
            if !dirs.contains(&system) {
                dirs.push(system);
            }
        }
    }
    std::env::join_paths(dirs).unwrap_or(inherited)
}

#[cfg(all(test, unix))]
mod tests;
