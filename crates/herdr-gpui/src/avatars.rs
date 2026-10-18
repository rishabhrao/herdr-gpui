use gpui::{Image, ImageFormat};
mod cache;
use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, SystemTime},
};

const MAX_REQUESTS: usize = 128;

/// A bound on each remote path segment, above GitHub's own name limits.
const MAX_SEGMENT: usize = 100;

/// Memory front end to the shared public-image disk cache.
pub struct Avatars {
    requests: mpsc::SyncSender<String>,
    results: mpsc::Receiver<(String, Option<Arc<Image>>)>,
    // Presence deduplicates pending requests as well as resolved hits and misses.
    images: HashMap<String, Option<Arc<Image>>>,
}

impl Avatars {
    pub fn new() -> Self {
        let (requests, incoming) = mpsc::sync_channel::<String>(MAX_REQUESTS);
        let (outgoing, results) = mpsc::sync_channel(MAX_REQUESTS * 2);
        let (refresh, network) = mpsc::sync_channel::<(String, String)>(MAX_REQUESTS);
        let updated = outgoing.clone();
        thread::spawn(move || {
            let agent = avatar_agent();
            let mut owners = HashMap::new();
            for (cwd, url) in network {
                let image = owners
                    .entry(url.clone())
                    .or_insert_with(|| download(&agent, &url))
                    .clone();
                if let Some(image) = image
                    && updated.send((cwd, Some(image))).is_err()
                {
                    break;
                }
            }
        });
        // Deliberately detach: dropping the sender ends the worker without a UI join.
        thread::spawn(move || {
            for cwd in incoming {
                let url = repo_owner(&cwd)
                    .map(|owner| format!("https://avatars.githubusercontent.com/{owner}?size=48"));
                let cached = url.as_deref().and_then(cached_image);
                let fresh = cached.as_ref().is_some_and(|(_, fresh)| *fresh);
                if outgoing
                    .send((cwd.clone(), cached.map(|(image, _)| image)))
                    .is_err()
                {
                    break;
                }
                if !fresh && let Some(url) = url {
                    let _ = refresh.try_send((cwd, url));
                }
            }
        });
        Self {
            requests,
            results,
            images: HashMap::new(),
        }
    }

    /// Enqueue each exact cwd at most once, including failed resolutions.
    pub fn request(&mut self, cwd: &str) {
        if self.images.len() >= MAX_REQUESTS {
            return;
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = self.images.entry(cwd.to_owned())
            && self.requests.try_send(entry.key().clone()).is_ok()
        {
            entry.insert(None);
        }
    }

    /// Drain completed requests without blocking. Misses also count as updates.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for (cwd, image) in self.results.try_iter() {
            self.images.insert(cwd, image);
            changed = true;
        }
        changed
    }

    pub fn image(&self, cwd: &str) -> Option<Arc<Image>> {
        self.images.get(cwd).cloned().flatten()
    }
}

fn repo_owner(cwd: &str) -> Option<String> {
    for key in ["remote.upstream.url", "remote.origin.url"] {
        if let Ok(output) = herdr_client::new_command("git")
            .args(["-C", cwd, "config", "--default", "", "--get", key])
            .output()
            && output.status.success()
            && let Ok(text) = std::str::from_utf8(&output.stdout)
            && let Some(owner) = github_owner(text.trim_end_matches(['\r', '\n']))
        {
            return Some(owner);
        }
    }
    None
}

fn github_owner(remote: &str) -> Option<String> {
    github_repo(remote).map(|(owner, _)| owner)
}

/// One segment of a remote's path.
///
/// Account and repository names are GitHub's to define, so no name grammar is
/// reproduced here: a segment is refused only when it names the directory
/// itself or carries punctuation that would not stay inside its own path
/// segment in the avatar URL built from it.
fn segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SEGMENT
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub(crate) fn github_repo(remote: &str) -> Option<(String, String)> {
    let path = ["git@github.com:", "ssh://git@github.com/"]
        .into_iter()
        .find_map(|prefix| remote.strip_prefix(prefix))
        .or_else(|| http_path(remote))?;
    let (owner, repo) = path.split_once('/')?;
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if !segment(owner) || !segment(repo) {
        return None;
    }
    Some((owner.to_ascii_lowercase(), repo.to_owned()))
}

/// The path of an `http(s)://[userinfo@]github.com/` remote.
///
/// Git accepts a user name, or a user name and token, before the host
/// (`https://user@github.com/o/r`, `https://user:token@github.com/o/r`).
/// The host is what Git connects to, so it alone must be `github.com`; the
/// userinfo is skipped and never kept. Userinfo that a URL parser could read
/// as the end of the authority (`/`, `?`, `#`, `\`, a second `@`) is refused
/// so the host cannot be misread.
fn http_path(remote: &str) -> Option<&str> {
    let rest = ["https://", "http://"]
        .into_iter()
        .find_map(|scheme| remote.strip_prefix(scheme))?;
    let (authority, path) = rest.split_once('/')?;
    let host = match authority.split_once('@') {
        None => authority,
        Some((userinfo, host)) => {
            let plain = |b: u8| b.is_ascii_graphic() && !matches!(b, b'@' | b'?' | b'#' | b'\\');
            if userinfo.is_empty() || !userinfo.bytes().all(plain) {
                return None;
            }
            host
        }
    };
    (host == "github.com").then_some(path)
}

fn cached_image(url: &str) -> Option<(Arc<Image>, bool)> {
    if !valid_profile_avatar(url) {
        return None;
    }
    cache::read(&cache::root()?, url, SystemTime::now())
}

pub(crate) type AvatarUpdates = mpsc::Receiver<Arc<Image>>;

/// Called by the profile worker after authenticated identity resolution.
pub(crate) fn profile_avatar(url: &str) -> (Option<Arc<Image>>, Option<AvatarUpdates>) {
    profile_avatar_with(cache::root(), url, SystemTime::now(), |url| {
        download(&avatar_agent(), &url)
    })
}

fn profile_avatar_with(
    root: Option<std::path::PathBuf>,
    url: &str,
    now: SystemTime,
    fetch: impl FnOnce(String) -> Option<Arc<Image>> + Send + 'static,
) -> (Option<Arc<Image>>, Option<AvatarUpdates>) {
    if !valid_profile_avatar(url) {
        return (None, None);
    }
    let cached = root.as_deref().and_then(|path| cache::read(path, url, now));
    let fresh = cached.as_ref().is_some_and(|(_, fresh)| *fresh);
    let image = cached.map(|(image, _)| image);
    if fresh {
        return (image, None);
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let url = url.to_owned();
    let _ = thread::Builder::new()
        .name("herdr-profile-avatar".into())
        .spawn(move || {
            if let Some(image) = fetch(url) {
                let _ = tx.send(image);
            }
        });
    (image, Some(rx))
}

fn avatar_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .max_redirects(0)
        .build()
        .into()
}

fn valid_profile_avatar(url: &str) -> bool {
    url.len() <= 2048
        && url
            .strip_prefix("https://avatars.githubusercontent.com/")
            .is_some_and(|path| {
                !path.is_empty()
                    && path
                        .bytes()
                        .all(|b| b.is_ascii_graphic() && !matches!(b, b'\\' | b'#'))
            })
}

fn download(agent: &ureq::Agent, url: &str) -> Option<Arc<Image>> {
    if !valid_profile_avatar(url) {
        return None;
    }
    let mut response = agent.get(url).call().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let content_type = response.headers().get("content-type")?.to_str().ok()?;
    let format = avatar_format(content_type)?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(cache::LIMIT as u64)
        .read_to_vec()
        .ok()?;
    let image = cache::decode(&bytes)?;
    if image.format != format {
        return None;
    }
    if let Some(root) = cache::root() {
        let _ = cache::write(&root, url, &bytes, SystemTime::now());
    }
    Some(image)
}

fn avatar_format(content_type: &str) -> Option<ImageFormat> {
    match content_type
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
