//! The HTTP routes past the front door: the page's files, pairing, and the
//! device list. Anything that changes state is a `POST` from the page's own
//! origin, so another site cannot ask on the user's behalf.
use super::{
    devices::{Claim, Device, Refusal},
    gate::Door,
    http::{Request, Response, set_cookie},
    hub::Hub,
};
use serde::Deserialize;
use serde_json::json;

/// The cookie name before cookies named their port, still read.
const LEGACY_DEVICE_COOKIE: &str = "herdr_device";

/// The cookie a paired device holds. Browsers share cookies between the
/// ports of one host, so the name carries the port: a second server on
/// `localhost` must never replace this one's cookie.
pub(super) fn device_cookie_name(port: u16) -> String {
    format!("herdr_device_{port}")
}

/// The cookie a browser holds while it waits for an approval.
fn pair_cookie_name(port: u16) -> String {
    format!("herdr_pair_{port}")
}
const DEVICE_MAX_AGE: u64 = 400 * 24 * 60 * 60;
const PAIR_MAX_AGE: u64 = 10 * 60;
/// The label this machine's own browser gets when it opens the token address.
const LOCAL_LABEL: &str = "This Mac";

/// The device a request comes from: any browser on this Mac, or a paired
/// device through the public door.
pub(super) fn device(hub: &Hub, request: &Request, door: Door) -> Option<Device> {
    let paired = paired(hub, request, door);
    if door == Door::Local {
        return Some(paired.unwrap_or_else(Device::this_mac));
    }
    paired
}

fn paired(hub: &Hub, request: &Request, door: Door) -> Option<Device> {
    let token = request
        .cookie(&device_cookie_name(hub.doors.port()))
        .or_else(|| request.cookie(LEGACY_DEVICE_COOKIE))?;
    let device = (token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| hub.devices.check(token))
        .flatten();
    if device.is_none() && door == Door::Public {
        // A browser that was paired and is now refused: say so where the
        // operator looks, so a lost pairing is never a mystery.
        eprintln!(
            "A browser sent a device cookie this server does not know ({} bytes); it must pair again.",
            token.len()
        );
    }
    device
}

fn device_cookie(hub: &Hub, token: &str, door: Door) -> String {
    set_cookie(
        &device_cookie_name(hub.doors.port()),
        token,
        DEVICE_MAX_AGE,
        door == Door::Public,
    )
}

fn clear(name: &str, door: Door) -> String {
    set_cookie(name, "", 0, door == Door::Public)
}

pub(super) fn get(hub: &Hub, request: &Request, door: Door) -> Response {
    match request.path.as_str() {
        "/" => index(hub, request, door),
        "/auth/status" => {
            let paired = device(hub, request, door);
            Response::json(
                200,
                &json!({
                    "paired": paired.is_some(),
                    "label": paired.map(|device| device.label),
                    "local": door == Door::Local,
                }),
            )
        }
        "/auth/devices" => {
            let Some(current) = device(hub, request, door) else {
                return Response::json(401, &json!({ "error": "not paired" }));
            };
            let devices: Vec<_> = hub
                .devices
                .list()
                .into_iter()
                .map(|device| {
                    json!({
                        "id": device.id,
                        "label": device.label,
                        "created": device.created,
                        "lastSeen": device.last_seen,
                        "remote": device.remote,
                        "current": device.id == current.id,
                    })
                })
                .collect();
            Response::json(200, &json!({ "devices": devices }))
        }
        "/pair/wait" => wait(hub, request, door),
        path if path.starts_with("/page/") => {
            if device(hub, request, door).is_none() {
                return Response::text(401, "Pair this device first");
            }
            page_file(hub, path).unwrap_or_else(|| Response::text(404, "Not found"))
        }
        path => asset(hub, request, path).unwrap_or_else(|| Response::text(404, "Not found")),
    }
}

fn index(hub: &Hub, request: &Request, door: Door) -> Response {
    // This machine's browser opened the address `serve` printed: it becomes
    // a paired device, and the token leaves the address bar.
    if door == Door::Local
        && let Some(token) = request.param("token")
    {
        if !hub.token_matches(token) {
            return Response::text(403, "This token is not the one `herdr-gpui serve` printed.");
        }
        if paired(hub, request, door).is_some() {
            return Response::redirect("/");
        }
        return match hub.devices.add(LOCAL_LABEL, false) {
            Ok((_, token)) => {
                Response::redirect("/").with("Set-Cookie", device_cookie(hub, &token, door))
            }
            Err(error) => {
                tracing::warn!(%error, "Could not pair this machine's browser");
                Response::text(503, "Could not save the paired device.")
            }
        };
    }
    asset(hub, request, "/")
        .unwrap_or_else(|| Response::text(404, "Not found"))
        .with(
            "Content-Security-Policy",
            hub.doors.content_security_policy(),
        )
        .with("X-Frame-Options", "DENY")
}

fn asset(hub: &Hub, request: &Request, path: &str) -> Option<Response> {
    let asset = hub.assets.get(path)?;
    if request
        .header("if-none-match")
        // Cloudflare's compression weakens the tag to `W/"..."`.
        .is_some_and(|tag| {
            tag.split(',')
                .any(|tag| tag.trim().trim_start_matches("W/") == asset.etag)
        })
    {
        return Some(
            Response::new(304, asset.kind, Vec::new())
                .revalidated()
                .with("ETag", asset.etag.clone()),
        );
    }
    Some(
        Response::new(200, asset.kind, asset.bytes.to_vec())
            .revalidated()
            .with("ETag", asset.etag.clone()),
    )
}

/// `/page/{id}/{secret}/{path}`: a local tab page's file.
fn page_file(hub: &Hub, path: &str) -> Option<Response> {
    let rest = path.strip_prefix("/page/")?;
    let (id, rest) = rest.split_once('/')?;
    let (secret, file) = rest.split_once('/').unwrap_or((rest, ""));
    let files = hub.page_files(id.parse().ok()?, secret)?;
    let (body, content_type) = files(&format!("/{file}"))?;
    Some(
        Response::new(200, content_type, body)
            .with("Content-Security-Policy", "frame-ancestors 'self'"),
    )
}

fn wait(hub: &Hub, request: &Request, door: Door) -> Response {
    let (Some(id), Some(secret)) = (
        request.param("id").and_then(|id| id.parse().ok()),
        request.cookie(&pair_cookie_name(hub.doors.port())),
    ) else {
        return Response::json(400, &json!({ "state": "gone" }));
    };
    match hub.devices.claim(id, secret) {
        Claim::Waiting => Response::json(200, &json!({ "state": "waiting" })),
        Claim::Approved(token) => Response::json(200, &json!({ "state": "approved" }))
            .with("Set-Cookie", device_cookie(hub, &token, door))
            .with(
                "Set-Cookie",
                clear(&pair_cookie_name(hub.doors.port()), door),
            ),
        Claim::Denied => Response::json(200, &json!({ "state": "denied" })).with(
            "Set-Cookie",
            clear(&pair_cookie_name(hub.doors.port()), door),
        ),
        Claim::Gone => Response::json(200, &json!({ "state": "gone" })),
    }
}

#[derive(Deserialize)]
struct Ask {
    #[serde(default)]
    label: String,
}

#[derive(Deserialize)]
struct Forget {
    id: String,
}

pub(super) fn post(hub: &Hub, request: &Request, door: Door, body: &[u8]) -> Response {
    if !hub.doors.origin_ok(request, door) {
        return Response::json(403, &json!({ "error": "another site" }));
    }
    match request.path.as_str() {
        "/pair/request" => {
            let Ok(ask) = serde_json::from_slice::<Ask>(body) else {
                return Response::json(400, &json!({ "error": "bad request" }));
            };
            match hub.devices.ask(&ask.label, door == Door::Public) {
                Ok(Ok((pairing, secret))) => {
                    let reply = json!({ "id": pairing.id, "code": pairing.code });
                    if door == Door::Local {
                        // A browser on this Mac is the Mac's own user, who
                        // could open the token address anyway: no one is
                        // asked, so the Mac can never lock itself out.
                        if let Err(error) = hub.devices.answer(pairing.id, true) {
                            tracing::warn!(%error, "Could not pair this Mac's browser");
                        }
                        println!("{} on this Mac may now use Herdr UI.", pairing.label);
                    } else {
                        println!(
                            "{} asks to use Herdr UI, code {}. Allow it in Herdr UI, or type here: allow {}",
                            pairing.label, pairing.code, pairing.code
                        );
                        hub.ask_approval(pairing);
                    }
                    Response::json(200, &reply).with(
                        "Set-Cookie",
                        set_cookie(&pair_cookie_name(hub.doors.port()), &secret, PAIR_MAX_AGE, door == Door::Public),
                    )
                }
                Ok(Err(Refusal::Busy)) => Response::json(
                    429,
                    &json!({ "error": "Other devices are already waiting. Try again in a few minutes." }),
                )
                .with("Retry-After", "60"),
                Ok(Err(Refusal::TooMany)) => Response::json(
                    429,
                    &json!({ "error": "Too many requests. Try again in ten minutes." }),
                )
                .with("Retry-After", "600"),
                Err(error) => {
                    tracing::warn!(%error, "Could not open a pairing request");
                    Response::json(503, &json!({ "error": "try again" }))
                }
            }
        }
        "/auth/forget" => {
            let Some(current) = device(hub, request, door) else {
                return Response::json(401, &json!({ "error": "not paired" }));
            };
            let Ok(forget) = serde_json::from_slice::<Forget>(body) else {
                return Response::json(400, &json!({ "error": "bad request" }));
            };
            forget_device(hub, &forget.id, current.id == forget.id, door)
        }
        "/auth/sign-out" => {
            let Some(current) = device(hub, request, door) else {
                return Response::json(200, &json!({ "ok": true })).with(
                    "Set-Cookie",
                    clear(&device_cookie_name(hub.doors.port()), door),
                );
            };
            forget_device(hub, &current.id, true, door)
        }
        _ => Response::json(404, &json!({ "error": "not found" })),
    }
}

fn forget_device(hub: &Hub, id: &str, current: bool, door: Door) -> Response {
    match hub.devices.forget(id) {
        Ok(removed) => {
            hub.drop_device(id);
            let response = Response::json(200, &json!({ "ok": removed }));
            if current {
                response.with(
                    "Set-Cookie",
                    clear(&device_cookie_name(hub.doors.port()), door),
                )
            } else {
                response
            }
        }
        Err(error) => {
            tracing::warn!(%error, "Could not forget a device");
            Response::json(503, &json!({ "error": "try again" }))
        }
    }
}
