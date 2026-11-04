//! The desktop app's way to the vxr Worker for the library ([`crate::sync`]): each [`Call`] on
//! its own thread over HTTPS (ureq over rustls), its [`Reply`] waiting for the window, which
//! `wake` brings round to take it ([`Courier::replies`]).

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use crate::sync::{Call, Reply};

/// The most of a reply's body read: over the largest file the library takes.
const BODY_LIMIT: u64 = 64 << 20;

/// How long a request body may take to send, besides [`SEND_PER_BYTE`]: the connection's
/// slow start and a pause or two.
const SEND_BASE: Duration = Duration::from_secs(30);

/// The slowest uplink an upload is given time for, in bytes a second (about 0.25 Mbit/s): a
/// file near the library's limit gets about 11 minutes. ureq times a body as a whole and has
/// no timeout for a stalled read, so a link slower than this, or stuck, gives up here.
const SEND_RATE: u64 = 32 << 10;

/// How long the Worker may take to answer once the request is in (the reply's headers), and
/// then to send the reply's body.
const WAIT_RESPONSE: Duration = Duration::from_secs(60);
const RECV_BODY: Duration = Duration::from_secs(15 * 60);

/// The time a request body of `len` bytes gets to go up: it grows with the file, so a big one
/// on a slow link is not cut off by a limit meant for a small one.
fn send_budget(len: usize) -> Duration {
    SEND_BASE + Duration::from_secs(len as u64 / SEND_RATE)
}

pub struct Courier {
    /// The Worker's origin, as `https://vxr.example.com`.
    server: String,
    agent: ureq::Agent,
    wake: Arc<dyn Fn() + Send + Sync>,
    tx: mpsc::Sender<(u32, Reply)>,
    rx: Mutex<mpsc::Receiver<(u32, Reply)>>,
}

impl Courier {
    /// Send calls to the Worker at `server`; `wake` runs on the call's thread once its reply
    /// is in.
    pub fn new(server: &str, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(WAIT_RESPONSE))
            .timeout_recv_body(Some(RECV_BODY))
            .build()
            .into();
        let (tx, rx) = mpsc::channel();
        Self { server: server.trim_end_matches('/').into(), agent, wake, tx, rx: Mutex::new(rx) }
    }

    pub fn send(&self, calls: Vec<Call>) {
        for call in calls {
            let (agent, url, tx, wake) =
                (self.agent.clone(), format!("{}{}", self.server, call.path), self.tx.clone(), self.wake.clone());
            std::thread::spawn(move || {
                let reply = fetch(&agent, &url, &call);
                if tx.send((call.id, reply)).is_ok() {
                    wake();
                }
            });
        }
    }

    /// The replies in since the last time, with the id of the call each answers.
    pub fn replies(&self) -> Vec<(u32, Reply)> {
        self.rx.lock().map(|rx| rx.try_iter().collect()).unwrap_or_default()
    }
}

/// Send `call` to `url`: status 0 when it did not get through.
fn fetch(agent: &ureq::Agent, url: &str, call: &Call) -> Reply {
    let sent = match call.method {
        "GET" => {
            let mut req = agent.get(url);
            for (k, v) in &call.headers {
                req = req.header(*k, v);
            }
            req.call()
        }
        method => {
            let mut req = if method == "PUT" { agent.put(url) } else { agent.post(url) };
            for (k, v) in &call.headers {
                req = req.header(*k, v);
            }
            // The body's time depends on its size, so each request sets its own.
            req.config().timeout_send_body(Some(send_budget(call.body.len()))).build().send(&call.body[..])
        }
    };
    let Ok(resp) = sent else { return Reply::default() };
    let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let (status, version, token) = (resp.status().as_u16(), header("x-vxr-version"), header("set-auth-token"));
    match resp.into_body().with_config().limit(BODY_LIMIT).read_to_vec() {
        Ok(body) => Reply { status, body, version, token },
        Err(_) => Reply::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_big_upload_gets_more_time_than_a_small_one() {
        assert_eq!(send_budget(0), SEND_BASE);
        // The library's largest file on a 0.25 Mbit/s uplink still fits; the old 120 s did not.
        assert!(send_budget(20 << 20) > Duration::from_secs(600));
        assert!(send_budget(20 << 20) > send_budget(1 << 20));
    }
}
