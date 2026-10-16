//! Which front door a request came through. The server listens on loopback
//! only; a request either comes from this machine's own browser, or through
//! a tunnel on this machine (such as `cloudflared`) for a public host the
//! operator named. Anything else is refused, so a page on a rebound DNS name
//! cannot reach the app.
use super::http::Request;

/// Headers only a proxy or tunnel adds. A loopback `Host` that carries one
/// is someone else's request forwarded here, never this machine's browser.
const FORWARDED: [&str; 6] = [
    "cf-connecting-ip",
    "cf-ray",
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-real-ip",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Door {
    /// This machine's own browser, at `localhost` or `127.0.0.1`.
    Local,
    /// A tunnel or proxy on this machine, for a named public host.
    Public,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Doors {
    port: u16,
    /// Lower case, without scheme, port, or path.
    public: Vec<String>,
}

/// `https://Example.com/` → `example.com`. `None` for an empty name.
pub(crate) fn host_name(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .unwrap_or(value);
    let value = value.split('/').next().unwrap_or_default();
    let value = value.strip_suffix(":443").unwrap_or(value);
    let valid = !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'));
    valid.then(|| value.to_ascii_lowercase())
}

impl Doors {
    pub(crate) fn new(port: u16, public: &[String]) -> Self {
        let mut public: Vec<String> = public.iter().filter_map(|host| host_name(host)).collect();
        public.sort();
        public.dedup();
        Self { port, public }
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn public_hosts(&self) -> &[String] {
        &self.public
    }

    fn local_hosts(&self) -> [String; 2] {
        [
            format!("localhost:{}", self.port),
            format!("127.0.0.1:{}", self.port),
        ]
    }

    pub(crate) fn door(&self, request: &Request) -> Option<Door> {
        let host = request.header("host")?.trim().to_ascii_lowercase();
        let forwarded = FORWARDED
            .iter()
            .any(|header| request.header(header).is_some());
        if self.local_hosts().contains(&host) {
            // A tunnel told to rewrite `Host` to loopback still forwards for
            // a remote browser, which must sign in like any other.
            return Some(if forwarded { Door::Public } else { Door::Local });
        }
        let host = host.strip_suffix(":443").unwrap_or(&host);
        self.public
            .iter()
            .any(|public| public == host)
            .then_some(Door::Public)
    }

    /// Whether `Origin` is this app's own page at a door: the check that
    /// keeps other sites from posting to it or opening its socket.
    pub(crate) fn origin_ok(&self, request: &Request, door: Door) -> bool {
        let Some(origin) = request.header("origin") else {
            return false;
        };
        let origin = origin.trim().to_ascii_lowercase();
        match door {
            Door::Local => self
                .local_hosts()
                .iter()
                .any(|host| origin == format!("http://{host}")),
            Door::Public => self
                .public
                .iter()
                .any(|host| origin == format!("https://{host}")),
        }
    }

    pub(crate) fn content_security_policy(&self) -> String {
        let [localhost, loopback] = self.local_hosts();
        let mut sockets = format!("ws://{localhost} ws://{loopback}");
        for host in &self.public {
            sockets.push_str(&format!(" wss://{host}"));
        }
        format!(
            "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; \
             img-src 'self' data: blob:; connect-src 'self' {sockets}; worker-src 'self'; \
             manifest-src 'self'; frame-src 'self' http: https:; base-uri 'none'; \
             form-action 'none'; frame-ancestors 'none'"
        )
    }
}
