//! `herdr-gpui serve`: its options, and the device commands that need no app.
use super::CliError;
use std::ffi::OsString;

/// The port `herdr-gpui serve` listens on unless `--port` names another.
pub const DEFAULT_SERVE_PORT: u16 = 1234;

/// What `serve` does: run the app, or manage paired devices and exit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ServeAction {
    #[default]
    Run,
    /// `serve devices`: list the paired browsers.
    Devices,
    /// `serve forget NAME`: unpair the browsers with this label or id.
    Forget(String),
}

/// `herdr-gpui serve [devices | forget NAME] [--port PORT]
/// [--public-host HOST]... [--access-team TEAM --access-aud TAG |
/// --no-access-check]`: the app with its windows in a browser tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeOptions {
    pub action: ServeAction,
    pub port: u16,
    /// Hosts a tunnel on this machine forwards to the server.
    pub public_hosts: Vec<String>,
    pub access_team: Option<String>,
    pub access_audience: Option<String>,
    /// Serves public hosts without checking Cloudflare Access's token.
    pub no_access_check: bool,
    port_given: bool,
}

const FLAGS: [&str; 5] = [
    "--port",
    "--public-host",
    "--access-team",
    "--access-aud",
    "--no-access-check",
];

/// Whether `flag` is one of `serve`'s own.
pub(super) fn takes(flag: &str) -> bool {
    FLAGS.contains(&flag)
}

/// Takes a leading `serve`, and the device command after it, if any.
pub(super) fn start(
    args: &mut std::iter::Peekable<impl Iterator<Item = OsString>>,
) -> Option<ServeOptions> {
    args.next_if(|arg| arg == "serve")?;
    let action = match args.peek().and_then(|arg| arg.to_str()) {
        Some("devices") => {
            args.next();
            ServeAction::Devices
        }
        Some("forget") => {
            args.next();
            // A missing name is caught when the options are checked.
            ServeAction::Forget(
                args.next_if(|arg| !arg.as_encoded_bytes().starts_with(b"-"))
                    .and_then(|name| name.into_string().ok())
                    .unwrap_or_default(),
            )
        }
        _ => ServeAction::Run,
    };
    Some(ServeOptions {
        action,
        port: DEFAULT_SERVE_PORT,
        public_hosts: Vec::new(),
        access_team: None,
        access_audience: None,
        no_access_check: false,
        port_given: false,
    })
}

fn value(
    flag: &'static str,
    args: &mut impl Iterator<Item = OsString>,
) -> Result<String, CliError> {
    args.next()
        .filter(|value| !value.is_empty() && !value.as_encoded_bytes().starts_with(b"-"))
        .ok_or(CliError::MissingValue(flag))?
        .into_string()
        .map_err(CliError::InvalidEncoding)
}

impl ServeOptions {
    pub(super) fn flag(
        &mut self,
        flag: &str,
        args: &mut impl Iterator<Item = OsString>,
    ) -> Result<(), CliError> {
        match flag {
            "--port" => {
                if std::mem::replace(&mut self.port_given, true) {
                    return Err(CliError::DuplicateOption("--port"));
                }
                self.port = value("--port", args)?
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0)
                    .ok_or(CliError::InvalidValue("--port"))?;
            }
            "--public-host" => self.public_hosts.push(value("--public-host", args)?),
            "--access-team" => {
                let team = value("--access-team", args)?;
                if self.access_team.replace(team).is_some() {
                    return Err(CliError::DuplicateOption("--access-team"));
                }
            }
            "--access-aud" => {
                let audience = value("--access-aud", args)?;
                if self.access_audience.replace(audience).is_some() {
                    return Err(CliError::DuplicateOption("--access-aud"));
                }
            }
            _ => self.no_access_check = true,
        }
        Ok(())
    }
}
