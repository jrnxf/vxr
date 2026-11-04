//! The assistant's native host: requests on their own threads with a blocking HTTPS client
//! (ureq over rustls, without gzip so streamed answers arrive as they are sent), and keys in
//! the OS keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux). The
//! account's session token is kept there too ([`Session`]).

use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use vxr_ai::net::{KeyPlace, Keys, Piece, Request, Stream, Transport};
use vxr_ai::provider::Kind;

#[derive(Default)]
pub struct Net {
    /// Write each answer's raw stream into this folder (the live check records fixtures).
    #[cfg(feature = "live")]
    pub record: Option<PathBuf>,
    #[cfg(feature = "live")]
    sent: usize,
}

impl Net {
    pub fn new() -> Net {
        Net::default()
    }
}

impl Transport for Net {
    fn send(&mut self, request: Request) -> Box<dyn Stream> {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        #[cfg(feature = "live")]
        let record = {
            self.sent += 1;
            self.record.as_ref().map(|d| d.join(format!("{:02}.sse", self.sent)))
        };
        #[cfg(not(feature = "live"))]
        let record: Option<PathBuf> = None;
        let flag = stop.clone();
        std::thread::spawn(move || fetch(request, tx, flag, record));
        Box::new(Thread { rx, stop })
    }
}

fn fetch(request: Request, tx: mpsc::Sender<Piece>, stop: Arc<AtomicBool>, record: Option<PathBuf>) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_per_read(Some(Duration::from_secs(vxr_ai::net::IDLE_SECS)))
        .build()
        .into();
    let mut req = agent.post(&request.url);
    for (k, v) in &request.headers {
        req = req.header(k, v);
    }
    let resp = match req.send(request.body.as_bytes()) {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Piece::Failed(e.to_string()));
            return;
        }
    };
    drop(request);
    let _ = tx.send(Piece::Status(resp.status().as_u16()));
    let mut file = record.and_then(|p| std::fs::File::create(p).ok());
    let mut body = resp.into_body();
    let mut reader = body.as_reader();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match reader.read(&mut buf) {
            Ok(0) => {
                let _ = tx.send(Piece::End);
                return;
            }
            Ok(n) => {
                if let Some(f) = &mut file {
                    let _ = std::io::Write::write_all(f, &buf[..n]);
                }
                if tx.send(Piece::Bytes(buf[..n].to_vec())).is_err() {
                    return;
                }
            }
            Err(e) => {
                let _ = tx.send(Piece::Failed(e.to_string()));
                return;
            }
        }
    }
}

struct Thread {
    rx: mpsc::Receiver<Piece>,
    stop: Arc<AtomicBool>,
}

impl Stream for Thread {
    fn poll(&mut self) -> Vec<Piece> {
        if self.stop.load(Ordering::Relaxed) {
            return Vec::new();
        }
        self.rx.try_iter().collect()
    }

    fn cancel(&mut self) {
        // The thread ends at its next read; the connection closes with it.
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Keys in the OS keychain, one entry per provider under the service "vxr".
pub struct Keychain;

fn entry(kind: Kind) -> Result<keyring_core::Entry, String> {
    keychain(&format!("{}-api-key", kind.id()))
}

impl Keys for Keychain {
    fn get(&mut self, kind: Kind) -> Option<String> {
        entry(kind).ok()?.get_password().ok().filter(|k| !k.is_empty())
    }

    fn set(&mut self, kind: Kind, key: &str, _remember: bool) -> Result<(), String> {
        entry(kind)?.set_password(key).map_err(|e| e.to_string())
    }

    fn remove(&mut self, kind: Kind) -> Result<(), String> {
        match entry(kind)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn has(&mut self, kind: Kind) -> bool {
        self.get(kind).is_some()
    }

    fn place(&self) -> KeyPlace {
        KeyPlace::Keychain
    }
}

/// The entry `account` under the service "vxr" in the OS keychain, set up the first time (as
/// `keyring::Entry::new` would); a store set before it (the tests' mock) is kept.
fn keychain(account: &str) -> Result<keyring_core::Entry, String> {
    _ = keyring::Entry::store_status();
    keyring_core::Entry::new("vxr", account).map_err(|e| e.to_string())
}

/// The account's session token in the OS keychain, under the service "vxr": the entry
/// `account-session`, or `account-session@<server>` for a Worker other than the default one
/// (so signing in to a local one leaves the real sign-in alone).
pub struct Session {
    account: String,
}

impl Session {
    /// The session for the Worker at `server`, `default` being the one the app ships with.
    pub fn new(server: &str, default: &str) -> Session {
        let account = if server == default { "account-session".into() } else { format!("account-session@{server}") };
        Session { account }
    }

    fn entry(&self) -> Result<keyring_core::Entry, String> {
        keychain(&self.account)
    }

    /// The token kept, if any.
    pub fn get(&self) -> Option<String> {
        self.entry().ok()?.get_password().ok().filter(|t| !t.is_empty())
    }

    /// Keep `token`, or delete the one kept (None).
    pub fn set(&self, token: Option<&str>) -> Result<(), String> {
        let entry = self.entry()?;
        match token {
            Some(t) => entry.set_password(t).map_err(|e| e.to_string()),
            None => match entry.delete_credential() {
                Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
                Err(e) => Err(e.to_string()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Session;

    #[test]
    fn session_token_kept_per_server_and_deleted() {
        // The mock store in place of the keychain: the real one is set up first, as the first
        // entry would, so it cannot replace the mock later.
        _ = keyring::Entry::store_status();
        keyring_core::set_default_store(keyring_core::mock::Store::new().expect("a mock store"));
        let default = "https://vxr.example.com";
        let (real, local) = (Session::new(default, default), Session::new("http://localhost:8787", default));
        assert_eq!(real.get(), None);
        real.set(Some("s1.sig")).expect("kept");
        local.set(Some("s2.sig")).expect("kept");
        assert_eq!(Session::new(default, default).get().as_deref(), Some("s1.sig"), "a relaunch finds it");
        local.set(None).expect("deleted");
        assert_eq!((real.get().as_deref(), local.get()), (Some("s1.sig"), None), "a local Worker's is its own");
        local.set(None).expect("deleting what is gone is fine");
    }
}
