//! The assistant's native host: requests on their own threads with a blocking HTTPS client
//! (ureq over rustls, without gzip so streamed answers arrive as they are sent), and keys in
//! the OS keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux).

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

fn entry(kind: Kind) -> Result<keyring::Entry, String> {
    keyring::Entry::new("vxr", &format!("{}-api-key", kind.id())).map_err(|e| e.to_string())
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
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
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
