//! Checks that a request through a public host passed Cloudflare Access.
//!
//! Access signs a JWT for each request it lets through and `cloudflared`
//! forwards it as `Cf-Access-Jwt-Assertion`. The token must be RS256-signed
//! by one of the team's current keys, name this application's audience, come
//! from the team's issuer, and not have expired. Every Access application of
//! a team signs with the same keys, so the audience check is what ties a
//! token to this one. Keys are fetched at start and every hour; until the
//! first fetch works, public requests are refused. With no application
//! given, the first properly signed token names it, and it is saved.
use base64::Engine as _;
use serde::Deserialize;
use std::{
    sync::{Arc, PoisonError, RwLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const REFRESH: Duration = Duration::from_secs(60 * 60);
const RETRY: Duration = Duration::from_secs(30);
const MAX_KEYS_BYTES: u64 = 256 * 1024;
const MAX_TOKEN_BYTES: usize = 16 * 1024;
/// Clocks a little apart still agree on a fresh token.
const LEEWAY: u64 = 60;

/// The Access application to check tokens against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessOptions {
    /// The team name, as in `<team>.cloudflareaccess.com`.
    pub team: String,
    /// The application's Audience (AUD) tag.
    pub audience: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Valid,
    /// No keys yet: the first fetch has not worked.
    Unready,
    Invalid(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    kid: String,
    modulus: Vec<u8>,
    exponent: Vec<u8>,
}

#[derive(Deserialize)]
struct KeySet {
    keys: Vec<RawKey>,
}

#[derive(Deserialize)]
struct RawKey {
    kid: String,
    kty: String,
    #[serde(default)]
    alg: Option<String>,
    n: String,
    e: String,
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct Claims {
    aud: Audience,
    iss: String,
    exp: u64,
    #[serde(default)]
    nbf: Option<u64>,
}

fn decode(value: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .ok()
}

pub(crate) fn parse_keys(json: &[u8]) -> Option<Vec<Key>> {
    let set: KeySet = serde_json::from_slice(json).ok()?;
    let keys: Vec<Key> = set
        .keys
        .into_iter()
        .filter(|key| key.kty == "RSA" && key.alg.as_deref().is_none_or(|alg| alg == "RS256"))
        .filter_map(|key| {
            Some(Key {
                kid: key.kid,
                modulus: decode(&key.n)?,
                exponent: decode(&key.e)?,
            })
        })
        .collect();
    (!keys.is_empty()).then_some(keys)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Checks `token` against `keys` for `options` at time `now`.
pub(crate) fn verify(token: &str, keys: &[Key], options: &AccessOptions, now: u64) -> Verdict {
    if token.len() > MAX_TOKEN_BYTES {
        return Verdict::Invalid("token too large");
    }
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Verdict::Invalid("not a JWT");
    };
    let Some(header_json) = decode(header) else {
        return Verdict::Invalid("bad header");
    };
    let Ok(header_fields) = serde_json::from_slice::<Header>(&header_json) else {
        return Verdict::Invalid("bad header");
    };
    if header_fields.alg != "RS256" {
        return Verdict::Invalid("not RS256");
    }
    let Some(key) = keys.iter().find(|key| key.kid == header_fields.kid) else {
        return Verdict::Invalid("unknown key");
    };
    let Some(signature) = decode(signature) else {
        return Verdict::Invalid("bad signature");
    };
    let public = ring::signature::RsaPublicKeyComponents {
        n: &key.modulus,
        e: &key.exponent,
    };
    let signed = format!("{header}.{payload}");
    if public
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            signed.as_bytes(),
            &signature,
        )
        .is_err()
    {
        return Verdict::Invalid("signature does not match");
    }
    let Some(claims) =
        decode(payload).and_then(|json| serde_json::from_slice::<Claims>(&json).ok())
    else {
        return Verdict::Invalid("bad claims");
    };
    let audience_ok = match &claims.aud {
        Audience::One(audience) => audience == &options.audience,
        Audience::Many(audiences) => audiences.contains(&options.audience),
    };
    if !audience_ok {
        return Verdict::Invalid("another application's token");
    }
    if claims.iss != format!("https://{}.cloudflareaccess.com", options.team) {
        return Verdict::Invalid("another team's token");
    }
    if claims.exp + LEEWAY < now {
        return Verdict::Invalid("expired");
    }
    if claims.nbf.is_some_and(|nbf| nbf > now + LEEWAY) {
        return Verdict::Invalid("not yet valid");
    }
    Verdict::Valid
}

/// Fetches a team's current keys; the network in use, a fixture in tests.
pub(crate) type Fetch = Box<dyn Fn(&str) -> Option<Vec<Key>> + Send + Sync>;

#[derive(Default)]
struct State {
    options: Option<AccessOptions>,
    keys: Option<Vec<Key>>,
}

pub(crate) struct Access {
    state: RwLock<State>,
    /// Where a learned team and audience are kept.
    file: Option<std::path::PathBuf>,
    fetch: Fetch,
}

#[derive(serde::Serialize, Deserialize)]
struct Saved {
    team: String,
    audience: String,
}

fn fetch_keys(team: &str) -> Option<Vec<Key>> {
    let url = format!("https://{team}.cloudflareaccess.com/cdn-cgi/access/certs");
    let mut response = match ureq::get(&url).call() {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "Could not fetch the Cloudflare Access keys");
            return None;
        }
    };
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_KEYS_BYTES)
        .read_to_vec()
        .ok()?;
    let keys = parse_keys(&body);
    if keys.is_none() {
        tracing::warn!("The Cloudflare Access key set has no RS256 key");
    }
    keys
}

/// A team name fit for a URL: letters, digits, and dashes.
fn plain_team(team: &str) -> bool {
    !team.is_empty()
        && team.len() <= 63
        && team
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

impl Access {
    /// Checks tokens for `options`, or for the application saved in `file`,
    /// or, with neither, for the first application whose properly signed
    /// token arrives, which is then saved. Keys refresh in the background.
    pub(crate) fn start(
        options: Option<AccessOptions>,
        file: Option<std::path::PathBuf>,
    ) -> crate::Result<Arc<Self>> {
        let access = Self::with_fetch(options, file, Box::new(fetch_keys));
        let worker = access.clone();
        thread::Builder::new()
            .name("web-access-keys".into())
            .spawn(move || {
                loop {
                    let team = worker
                        .state
                        .read()
                        .unwrap_or_else(PoisonError::into_inner)
                        .options
                        .as_ref()
                        .map(|options| options.team.clone());
                    let wait = match team.and_then(|team| (worker.fetch)(&team)) {
                        Some(keys) => {
                            worker
                                .state
                                .write()
                                .unwrap_or_else(PoisonError::into_inner)
                                .keys = Some(keys);
                            REFRESH
                        }
                        // A failed refresh keeps the keys already held; an
                        // application not learned yet is looked at again soon.
                        None => RETRY,
                    };
                    thread::sleep(wait);
                }
            })?;
        Ok(access)
    }

    pub(crate) fn with_fetch(
        options: Option<AccessOptions>,
        file: Option<std::path::PathBuf>,
        fetch: Fetch,
    ) -> Arc<Self> {
        let options = options.or_else(|| {
            let saved: Saved = serde_json::from_slice(&std::fs::read(file.as_ref()?).ok()?).ok()?;
            plain_team(&saved.team).then_some(AccessOptions {
                team: saved.team,
                audience: saved.audience,
            })
        });
        Arc::new(Self {
            state: RwLock::new(State {
                options,
                keys: None,
            }),
            file,
            fetch,
        })
    }

    #[cfg(test)]
    pub(crate) fn learned(&self) -> Option<AccessOptions> {
        self.state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .options
            .clone()
    }

    pub(crate) fn check(&self, token: Option<&str>) -> Verdict {
        let Some(token) = token else {
            return Verdict::Invalid("no Access token");
        };
        {
            let state = self.state.read().unwrap_or_else(PoisonError::into_inner);
            if let Some(options) = &state.options {
                return match &state.keys {
                    Some(keys) => verify(token, keys, options, now()),
                    None => Verdict::Unready,
                };
            }
        }
        self.learn(token)
    }

    /// Takes the application from a token that only Cloudflare could have
    /// signed for it. Only Cloudflare's edge reaches this server through the
    /// tunnel, so the first such token is the one the operator's own Access
    /// application issued.
    fn learn(&self, token: &str) -> Verdict {
        let Some((team, audience)) = hint(token) else {
            return Verdict::Invalid("not an Access token");
        };
        if !plain_team(&team) {
            return Verdict::Invalid("not an Access token");
        }
        let Some(keys) = (self.fetch)(&team) else {
            return Verdict::Unready;
        };
        let options = AccessOptions { team, audience };
        let verdict = verify(token, &keys, &options, now());
        if verdict != Verdict::Valid {
            return verdict;
        }
        if let Some(file) = &self.file {
            let saved = Saved {
                team: options.team.clone(),
                audience: options.audience.clone(),
            };
            if let Err(error) = save(file, &saved) {
                tracing::warn!(%error, "Could not save the Cloudflare Access application");
            }
        }
        println!(
            "Learned the Cloudflare Access application ({}). Every request through the tunnel now needs its token.",
            options.team
        );
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        if state.options.is_none() {
            state.options = Some(options);
            state.keys = Some(keys);
        }
        Verdict::Valid
    }
}

fn save(path: &std::path::Path, saved: &Saved) -> crate::Result<()> {
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
    let bytes = serde_json::to_vec_pretty(saved).map_err(|_| crate::Error::BadRequest)?;
    file.write_all(&bytes)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

/// The team and audience an unchecked token names, to tell the operator
/// which values to configure. Never used to let a request in.
pub(crate) fn hint(token: &str) -> Option<(String, String)> {
    let payload = token.split('.').nth(1)?;
    let claims: Claims = serde_json::from_slice(&decode(payload)?).ok()?;
    let team = claims
        .iss
        .strip_prefix("https://")?
        .strip_suffix(".cloudflareaccess.com")?
        .to_owned();
    let audience = match claims.aud {
        Audience::One(audience) => audience,
        Audience::Many(audiences) => audiences.into_iter().next()?,
    };
    Some((team, audience))
}
