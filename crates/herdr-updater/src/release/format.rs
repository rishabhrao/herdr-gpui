//! The installed format chooses the signed asset, never the host distribution.
use super::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Format {
    Archive,
    Deb,
    Rpm,
    Arch,
    /// The per-user Windows `-setup.exe`, listed in its own signed manifest so
    /// installed macOS and Linux clients never see a target they reject.
    Installer,
}

/// The Windows targets a release publishes an installer for.
pub(crate) const WINDOWS_TARGETS: [&str; 2] = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"];

impl Format {
    pub(crate) fn name(self, version: &str, target: &str) -> Result<String> {
        match self {
            Self::Archive => super::asset_name(version, target),
            Self::Installer if WINDOWS_TARGETS.contains(&target) => {
                Ok(format!("Herdr-{version}-{target}-setup.exe"))
            }
            Self::Installer => Err(Error::UnsupportedTarget),
            Self::Deb | Self::Rpm | Self::Arch => {
                if !matches!(
                    target,
                    "x86_64-unknown-linux-gnu" | "aarch64-unknown-linux-gnu"
                ) {
                    return Err(Error::UnsupportedTarget);
                }
                let extension = match self {
                    Self::Deb => "deb",
                    Self::Rpm => "rpm",
                    _ => "pkg.tar.zst",
                };
                Ok(format!("Herdr-{version}-{target}.{extension}"))
            }
        }
    }

    pub(crate) fn manifest(self) -> (&'static str, &'static str) {
        match self {
            Self::Archive => ("update-manifest.json", "update-manifest.sig"),
            Self::Installer => ("windows-manifest.json", "windows-manifest.sig"),
            Self::Deb | Self::Rpm | Self::Arch => ("package-manifest.json", "package-manifest.sig"),
        }
    }

    /// The manifest schema that lists this format's assets.
    pub(crate) fn schema(self) -> u32 {
        match self {
            Self::Archive => 1,
            Self::Deb | Self::Rpm | Self::Arch => 2,
            Self::Installer => 3,
        }
    }
}
