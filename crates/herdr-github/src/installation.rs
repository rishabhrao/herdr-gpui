//! Whether a GitHub App sign-in can reach private repositories. Such a token
//! only reads what the App is installed on, with the permissions each owner
//! accepted. When the App asks for more, GitHub keeps every installation on
//! its old permissions until the owner approves, so the user must be told.
//! What the App asks for is read from GitHub, never kept here, so a later
//! change to the registration needs no change to this build.

use super::{
    Result,
    http::{agent, authorization, response},
    log,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::time::Duration;

/// Installations read from GitHub's first page; an account in more is rare,
/// and the rest are simply not judged.
const PAGE: usize = 100;

/// What a GitHub App sign-in still needs from the user.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Access {
    /// Not checked yet, not a GitHub App token, or the check failed.
    #[default]
    Unknown,
    /// Every installation the account can see has the App's permissions.
    Ready,
    /// No installation reaches the account, so only public repositories load.
    NotInstalled,
    /// Owners that have not approved the App's current permissions.
    Pending(Vec<Installation>),
}

impl Access {
    /// Whether the account shows the attention dot and its GitHub page row.
    pub fn needs_attention(&self) -> bool {
        matches!(self, Self::NotInstalled | Self::Pending(_))
    }
}

/// One installation waiting for its owner to approve new permissions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installation {
    pub owner: String,
    pub organization: bool,
    id: u64,
}

impl Installation {
    #[cfg(any(test, feature = "test-support"))]
    pub fn fixture(owner: &str, organization: bool, id: u64) -> Self {
        Self {
            owner: owner.into(),
            organization,
            id,
        }
    }

    /// Where the owner reviews and approves the request. Built from the id
    /// and a checked login rather than taken from the response.
    pub fn url(&self) -> String {
        if self.organization {
            format!(
                "https://github.com/organizations/{}/settings/installations/{}",
                self.owner, self.id
            )
        } else {
            format!("https://github.com/settings/installations/{}", self.id)
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Listing {
    #[serde(default)]
    installations: Vec<Listed>,
}

#[derive(Deserialize)]
struct Listed {
    id: u64,
    app_slug: String,
    account: Owner,
    #[serde(default)]
    permissions: Map<String, Value>,
}

#[derive(Deserialize)]
struct Owner {
    login: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
pub(super) struct App {
    #[serde(default)]
    permissions: Map<String, Value>,
}

/// The access of a GitHub App user token (`ghu_`). Any other token reaches
/// repositories by its own grants, so it is [`Access::Unknown`] unchecked.
pub(super) fn check(token: &SecretString) -> Result<Access> {
    if !app_token(token) {
        return Ok(Access::Unknown);
    }
    let listing: Listing = get(
        "installations",
        token,
        &format!("https://api.github.com/user/installations?per_page={PAGE}"),
    )?;
    // Every installation a user token lists belongs to the token's own App.
    let Some(slug) = listing.installations.first().map(|i| i.app_slug.as_str()) else {
        return Ok(Access::NotInstalled);
    };
    if !crate::config::valid_owner(slug) {
        tracing::warn!(
            category = "github_installation",
            "GitHub listed an installation with an unexpected App slug"
        );
        return Ok(Access::Unknown);
    }
    let app: App = get("app", token, &format!("https://api.github.com/apps/{slug}"))?;
    Ok(assess(&listing, &app))
}

/// Whether `token` is a GitHub App user token, the only kind checked.
pub(super) fn app_token(token: &SecretString) -> bool {
    token.expose_secret().starts_with("ghu_")
}

/// Compares each installation with what the App asks for. Pure, so the
/// comparison is testable without GitHub.
pub(super) fn assess(listing: &Listing, app: &App) -> Access {
    let mut pending: Vec<Installation> = listing
        .installations
        .iter()
        .take(PAGE)
        .filter(|installation| crate::config::valid_owner(&installation.account.login))
        .map(|installation| {
            let organization = installation.account.kind == "Organization";
            (installation, organization)
        })
        .filter(|(installation, organization)| {
            app.permissions
                .iter()
                .filter(|(name, _)| *organization || !organization_only(name))
                .any(|(name, wanted)| {
                    level(wanted) > installation.permissions.get(name.as_str()).map_or(0, level)
                })
        })
        .map(|(installation, organization)| Installation {
            owner: installation.account.login.clone(),
            organization,
            id: installation.id,
        })
        .collect();
    if pending.is_empty() {
        return Access::Ready;
    }
    // The user's own account first: the one they can always approve.
    pending.sort_by(|a, b| (a.organization, &a.owner).cmp(&(b.organization, &b.owner)));
    tracing::info!(
        category = "github_installation",
        pending = pending.len(),
        "GitHub App installations are waiting for permission approval"
    );
    Access::Pending(pending)
}

/// Organization permissions, which an installation on a personal account
/// never receives however its owner answers.
fn organization_only(name: &str) -> bool {
    name.starts_with("organization_") || matches!(name, "members" | "team_discussions")
}

/// GitHub's permission levels in order; one this build does not know ranks
/// highest, so a newer level is never mistaken for an approved one.
fn level(value: &Value) -> u8 {
    match value.as_str() {
        Some("read") => 1,
        Some("write") => 2,
        Some("admin") => 3,
        _ => u8::MAX,
    }
}

fn get<T: serde::de::DeserializeOwned>(
    context: &'static str,
    token: &SecretString,
    url: &str,
) -> Result<T> {
    response(
        context,
        agent(Duration::from_secs(15))
            .get(url)
            .header("User-Agent", "Herdr-GPUI")
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", authorization(token)?)
            .call()
            .map_err(log::network(context))?,
    )
}
