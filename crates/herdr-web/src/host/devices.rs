//! Browsers allowed to drive the app, and browsers asking to.
//!
//! A device holds a 256-bit token in an `HttpOnly` cookie; this side keeps
//! only its SHA-256, in an owner-only file read again whenever it changes, so
//! `herdr-gpui serve forget` takes effect on the running server. A new
//! device asks with a short code; someone already using the app approves it
//! there after checking the codes match.
use super::hub::{random_hex, same_secret};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) const FILE: &str = "web-devices.json";
/// How long a request waits for an answer, and its code stays good.
const REQUEST_LIFETIME: Duration = Duration::from_secs(10 * 60);
/// Requests waiting at once; more are refused until one is answered.
const MAX_PENDING: usize = 4;
/// Requests allowed per window, from every browser together.
const MAX_REQUESTS: usize = 12;
const REQUEST_WINDOW: Duration = Duration::from_secs(10 * 60);
/// A device's last use is written at most this often.
const SEEN_EVERY: u64 = 60;
/// No glyph pair a person confuses: no 0/O, 1/I/L, U/V.
const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTWXYZ";
const MAX_LABEL: usize = 48;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub label: String,
    token_sha256: String,
    /// Seconds since the Unix epoch.
    pub created: u64,
    pub last_seen: u64,
    /// Paired through a tunnel rather than at this machine.
    pub remote: bool,
}

impl Device {
    /// A browser on this Mac. It needs no pairing: whoever uses this Mac
    /// can drive the app already.
    pub(crate) fn this_mac() -> Self {
        Self {
            id: "this-mac".into(),
            label: "This Mac".into(),
            token_sha256: String::new(),
            created: 0,
            last_seen: 0,
            remote: false,
        }
    }
}

/// A browser asking to drive the app, for someone to approve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingRequest {
    pub id: u64,
    /// Shown on the asking device too, so the approver can match them.
    pub code: String,
    pub label: String,
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    Waiting,
    /// The new device's token, until the asking browser collects it.
    Approved(String),
    Denied,
}

struct Pending {
    request: PairingRequest,
    secret_sha256: String,
    remote: bool,
    asked: Instant,
    state: State,
}

/// What the asking browser learns when it checks back.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Claim {
    Waiting,
    /// Its device token, handed over once.
    Approved(String),
    Denied,
    /// No such request, a wrong secret, or one that ran out of time.
    Gone,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    Busy,
    TooMany,
}

#[derive(Default)]
struct Cache {
    modified: Option<SystemTime>,
    devices: Vec<Device>,
}

pub(crate) struct Devices {
    path: PathBuf,
    cache: Mutex<Cache>,
    pending: Mutex<Vec<Pending>>,
    asked: Mutex<VecDeque<Instant>>,
    next: Mutex<u64>,
}

pub(crate) fn sha256_hex(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A label fit to show in a dialog: printable, trimmed, bounded.
pub(crate) fn clean_label(label: &str) -> String {
    let label: String = label
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_LABEL)
        .collect();
    let label = label.trim();
    if label.is_empty() {
        "A browser".to_owned()
    } else {
        label.to_owned()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn code() -> crate::Result<String> {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes).map_err(|_| crate::Error::Random)?;
    let letters: String = bytes
        .iter()
        .map(|byte| char::from(CODE_ALPHABET[usize::from(*byte) % CODE_ALPHABET.len()]))
        .collect();
    Ok(format!("{}-{}", &letters[..3], &letters[3..]))
}

/// Reads the registry. A file that cannot be parsed holds no devices: a
/// broken registry fails closed.
pub(crate) fn read(path: &Path) -> Vec<Device> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            tracing::warn!(%error, "The web device list cannot be read; no device is paired");
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

/// Writes the registry owner-only, replacing it whole.
pub(crate) fn write(path: &Path, devices: &[Device]) -> crate::Result<()> {
    use std::{fs::OpenOptions, io::Write as _, os::unix::fs::OpenOptionsExt as _};
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&temporary);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let bytes = serde_json::to_vec_pretty(devices).map_err(|_| crate::Error::BadRequest)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

/// Removes the devices whose id or label is `name`; how many went.
pub fn forget(state_dir: &Path, name: &str) -> crate::Result<usize> {
    let path = state_dir.join(FILE);
    let mut devices = read(&path);
    let before = devices.len();
    devices.retain(|device| device.id != name && device.label != name);
    let removed = before - devices.len();
    if removed > 0 {
        write(&path, &devices)?;
    }
    Ok(removed)
}

/// Every paired device.
pub fn list(state_dir: &Path) -> Vec<Device> {
    read(&state_dir.join(FILE))
}

impl Devices {
    pub(crate) fn new(state_dir: &Path) -> Self {
        Self {
            path: state_dir.join(FILE),
            cache: Mutex::default(),
            pending: Mutex::default(),
            asked: Mutex::default(),
            next: Mutex::new(1),
        }
    }

    /// The registry, read again only when the file changed.
    fn current(&self) -> MutexGuard<'_, Cache> {
        let mut cache = lock(&self.cache);
        let modified = std::fs::metadata(&self.path)
            .and_then(|metadata| metadata.modified())
            .ok();
        if modified != cache.modified || modified.is_none() {
            cache.devices = read(&self.path);
            cache.modified = modified;
        }
        cache
    }

    fn save(&self, cache: &mut Cache) -> crate::Result<()> {
        write(&self.path, &cache.devices)?;
        cache.modified = std::fs::metadata(&self.path)
            .and_then(|metadata| metadata.modified())
            .ok();
        Ok(())
    }

    /// The device `token` belongs to, noting that it was seen.
    pub(crate) fn check(&self, token: &str) -> Option<Device> {
        let hash = sha256_hex(token);
        let mut cache = self.current();
        let index = cache
            .devices
            .iter()
            .position(|device| same_secret(&device.token_sha256, &hash))?;
        let stamp = now();
        if stamp.saturating_sub(cache.devices[index].last_seen) >= SEEN_EVERY {
            cache.devices[index].last_seen = stamp;
            let _ = self.save(&mut cache);
        }
        Some(cache.devices[index].clone())
    }

    /// Pairs a new device; its token, which only it keeps.
    pub(crate) fn add(&self, label: &str, remote: bool) -> crate::Result<(Device, String)> {
        let token = random_hex(32)?;
        let stamp = now();
        let device = Device {
            id: random_hex(6)?,
            label: clean_label(label),
            token_sha256: sha256_hex(&token),
            created: stamp,
            last_seen: stamp,
            remote,
        };
        let mut cache = self.current();
        cache.devices.push(device.clone());
        self.save(&mut cache)?;
        Ok((device, token))
    }

    pub(crate) fn list(&self) -> Vec<Device> {
        self.current().devices.clone()
    }

    pub(crate) fn forget(&self, id: &str) -> crate::Result<bool> {
        let mut cache = self.current();
        let before = cache.devices.len();
        cache.devices.retain(|device| device.id != id);
        let removed = cache.devices.len() != before;
        if removed {
            self.save(&mut cache)?;
        }
        Ok(removed)
    }

    fn sweep(pending: &mut Vec<Pending>) {
        pending.retain(|request| request.asked.elapsed() < REQUEST_LIFETIME);
    }

    /// Opens a request: the request to show the approver, and the secret
    /// the asking browser keeps to collect its answer.
    pub(crate) fn ask(
        &self,
        label: &str,
        remote: bool,
    ) -> crate::Result<Result<(PairingRequest, String), Refusal>> {
        {
            let mut asked = lock(&self.asked);
            while asked
                .front()
                .is_some_and(|at| at.elapsed() >= REQUEST_WINDOW)
            {
                asked.pop_front();
            }
            if asked.len() >= MAX_REQUESTS {
                return Ok(Err(Refusal::TooMany));
            }
            asked.push_back(Instant::now());
        }
        let mut pending = lock(&self.pending);
        Self::sweep(&mut pending);
        if pending
            .iter()
            .filter(|request| request.state == State::Waiting)
            .count()
            >= MAX_PENDING
        {
            return Ok(Err(Refusal::Busy));
        }
        let id = {
            let mut next = lock(&self.next);
            let id = *next;
            *next += 1;
            id
        };
        let secret = random_hex(16)?;
        let request = PairingRequest {
            id,
            code: code()?,
            label: clean_label(label),
        };
        pending.push(Pending {
            request: request.clone(),
            secret_sha256: sha256_hex(&secret),
            remote,
            asked: Instant::now(),
            state: State::Waiting,
        });
        Ok(Ok((request, secret)))
    }

    /// The approver's answer. Allowing pairs the device at once; its token
    /// waits for the asking browser.
    pub(crate) fn answer(&self, id: u64, allow: bool) -> crate::Result<bool> {
        let mut pending = lock(&self.pending);
        Self::sweep(&mut pending);
        let Some(request) = pending
            .iter_mut()
            .find(|request| request.request.id == id && request.state == State::Waiting)
        else {
            return Ok(false);
        };
        request.state = if allow {
            let (_, token) = self.add(&request.request.label, request.remote)?;
            State::Approved(token)
        } else {
            State::Denied
        };
        Ok(true)
    }

    pub(crate) fn is_waiting(&self, id: u64) -> bool {
        let mut pending = lock(&self.pending);
        Self::sweep(&mut pending);
        pending
            .iter()
            .any(|request| request.request.id == id && request.state == State::Waiting)
    }

    /// The waiting request with `code`, typed with or without its dash.
    pub(crate) fn waiting_with_code(&self, code: &str) -> Option<u64> {
        let wanted: String = code
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|character| character.to_ascii_uppercase())
            .collect();
        let mut pending = lock(&self.pending);
        Self::sweep(&mut pending);
        pending
            .iter()
            .find(|request| {
                request.state == State::Waiting && request.request.code.replace('-', "") == wanted
            })
            .map(|request| request.request.id)
    }

    /// What the browser holding `secret` learns about request `id`.
    pub(crate) fn claim(&self, id: u64, secret: &str) -> Claim {
        let mut pending = lock(&self.pending);
        Self::sweep(&mut pending);
        let hash = sha256_hex(secret);
        let Some(index) = pending.iter().position(|request| {
            request.request.id == id && same_secret(&request.secret_sha256, &hash)
        }) else {
            return Claim::Gone;
        };
        match &pending[index].state {
            State::Waiting => Claim::Waiting,
            State::Approved(_) | State::Denied => match pending.remove(index).state {
                State::Approved(token) => Claim::Approved(token),
                _ => Claim::Denied,
            },
        }
    }
}
