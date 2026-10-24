//! Tells Herdr what the pane's agent is doing, and how Herdr UI can reach it.
//!
//! Reports go through Herdr's JSON API on a worker thread from a mailbox that
//! keeps only the latest activity, so a slow daemon never holds up the chat.

use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use crate::{Error, Result, session::Activity};

/// The report source, so Herdr can tell these reports from its own detection.
const SOURCE: &str = "herdr-acp";
/// The agent label Herdr records for the pane.
const AGENT: &str = "acp";
const TIMEOUT: Duration = Duration::from_secs(2);
/// How long a failed announcement waits before it is sent again.
const RETRY: Duration = Duration::from_secs(2);
const MAX_RESPONSE: u64 = 64 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub pane: String,
    pub socket: PathBuf,
}

impl Target {
    /// The pane this process runs in, when Herdr started it.
    pub(crate) fn from_env() -> Option<Self> {
        let pane = std::env::var("HERDR_PANE_ID")
            .ok()
            .filter(|v| !v.is_empty())?;
        let socket = std::env::var_os("HERDR_SOCKET_PATH").filter(|v| !v.is_empty())?;
        Some(Self {
            pane,
            socket: socket.into(),
        })
    }
}

#[derive(Default)]
struct Mailbox {
    activity: Option<Activity>,
    closing: bool,
}

/// Replaces the activity waiting to be reported.
pub(crate) struct Report(Arc<(Mutex<Mailbox>, Condvar)>);

impl Report {
    pub(crate) fn report(&self, activity: Activity) {
        let (lock, wake) = &*self.0;
        lock.lock().unwrap_or_else(PoisonError::into_inner).activity = Some(activity);
        wake.notify_one();
    }
}

pub(crate) struct Reporter {
    handle: Option<thread::JoinHandle<()>>,
    mailbox: Arc<(Mutex<Mailbox>, Condvar)>,
}

impl Reporter {
    /// Starts reporting for `target`; `endpoint` is the token Herdr UI finds the
    /// chat by, absent when the chat does not listen.
    pub(crate) fn start(target: Target, display: String, endpoint: Option<String>) -> Result<Self> {
        let mailbox = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let shared = Arc::clone(&mailbox);
        let handle = thread::Builder::new()
            .name("herdr-report".to_owned())
            .spawn(move || run(&target, &display, endpoint.as_deref(), &shared))?;
        Ok(Self {
            handle: Some(handle),
            mailbox,
        })
    }

    /// A handle the session reports through.
    pub(crate) fn handle(&self) -> Report {
        Report(Arc::clone(&self.mailbox))
    }

    /// Withdraws the pane's agent and token, then waits briefly for Herdr.
    pub(crate) fn close(mut self) {
        let (lock, wake) = &*self.mailbox;
        lock.lock().unwrap_or_else(PoisonError::into_inner).closing = true;
        wake.notify_one();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run(
    target: &Target,
    display: &str,
    endpoint: Option<&str>,
    mailbox: &(Mutex<Mailbox>, Condvar),
) {
    let mut seq = Sequence::default();
    // Whether the agent was reported, so closing withdraws it, and whether
    // the token reached Herdr: until it has, the native view cannot find
    // the chat, so a failed announcement is sent again after `RETRY`.
    let mut reported = false;
    let mut announced = false;
    loop {
        let (activity, closing) = {
            let (lock, wake) = mailbox;
            let mut state = lock.lock().unwrap_or_else(PoisonError::into_inner);
            while state.activity.is_none() && !state.closing {
                if reported && !announced {
                    let (next, waited) = wake
                        .wait_timeout(state, RETRY)
                        .unwrap_or_else(PoisonError::into_inner);
                    state = next;
                    if waited.timed_out() {
                        break;
                    }
                } else {
                    state = wake.wait(state).unwrap_or_else(PoisonError::into_inner);
                }
            }
            (state.activity.take(), state.closing)
        };
        if let Some(activity) = activity {
            let state = match activity {
                Activity::Idle => "idle",
                Activity::Working => "working",
                Activity::Blocked => "blocked",
            };
            let _ = call(
                &target.socket,
                "pane.report_agent",
                json!({"pane_id": target.pane, "source": SOURCE, "agent": AGENT,
                    "state": state, "seq": seq.next()}),
            );
            reported = true;
        }
        if reported && !announced && !closing {
            let mut tokens = serde_json::Map::new();
            if let Some(endpoint) = endpoint {
                tokens.insert(herdr_chat_protocol::TOKEN.to_owned(), json!(endpoint));
            }
            announced = call(
                &target.socket,
                "pane.report_metadata",
                json!({"pane_id": target.pane, "source": SOURCE, "agent": AGENT,
                    "display_agent": display, "tokens": tokens, "seq": seq.next()}),
            )
            .is_ok();
        }
        if closing {
            if reported {
                let _ = call(
                    &target.socket,
                    "pane.report_metadata",
                    json!({"pane_id": target.pane, "source": SOURCE, "agent": AGENT,
                        "tokens": {herdr_chat_protocol::TOKEN: null}, "clear_display_agent": true,
                        "seq": seq.next()}),
                );
                let _ = call(
                    &target.socket,
                    "pane.release_agent",
                    json!({"pane_id": target.pane, "source": SOURCE, "agent": AGENT,
                        "seq": seq.next()}),
                );
            }
            return;
        }
    }
}

/// Report sequence numbers that keep increasing across restarts in one pane,
/// since Herdr ignores a report older than the last one from its source.
#[derive(Default)]
struct Sequence(u64);

impl Sequence {
    fn next(&mut self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
            });
        self.0 = now.max(self.0 + 1);
        self.0
    }
}

fn call(socket: &std::path::Path, method: &str, params: Value) -> Result<()> {
    let mut stream = connect(socket)?;
    let request = json!({"id": format!("herdr-acp-{method}"), "method": method, "params": params});
    let mut line = serde_json::to_vec(&request).map_err(Error::Encode)?;
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()?;
    let mut response = String::new();
    BufReader::new(stream.take(MAX_RESPONSE)).read_line(&mut response)?;
    let response: Value = serde_json::from_str(&response).map_err(Error::Encode)?;
    match response.get("error") {
        Some(error) => Err(Error::HerdrApi(error.to_string())),
        None => Ok(()),
    }
}

/// Herdr's API endpoint: a socket on Unix, and on Windows the named pipe
/// Herdr derives from the same path, which `herdr_client::Stream` maps alike.
fn connect(socket: &std::path::Path) -> Result<herdr_client::Stream> {
    // Only the Windows stream needs `&mut` to set its timeouts.
    #[cfg_attr(unix, allow(unused_mut))]
    let mut stream = herdr_client::Stream::connect(socket)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    Ok(stream)
}

#[cfg(test)]
mod tests;
