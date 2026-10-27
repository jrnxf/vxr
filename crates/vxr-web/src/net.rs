//! The assistant in the browser: requests go straight from the page to the provider with
//! `fetch`, their streamed bodies read chunk by chunk (no proxy, `spikes/ai-browser-calls`),
//! and keys live in this tab's memory, or encrypted in the browser when the user asks
//! (`window.vxrKeys` in the page: a non-extractable AES-GCM key in IndexedDB).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use js_sys::{Function, Reflect, Uint8Array};
use vxr_ai::net::{KeyPlace, Keys, Piece, Request, Stream, Transport};
use vxr_ai::provider::Kind;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// The hosts the page's content security policy lets it reach (`www/_headers`), besides its
/// own origin.
pub const HOSTS: [&str; 7] = [
    "https://api.anthropic.com",
    "https://api.openai.com",
    "https://openrouter.ai",
    "http://127.0.0.1:11434",
    "http://localhost:11434",
    "http://127.0.0.1:1234",
    "http://localhost:1234",
];

fn origin(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    format!("{scheme}://{}", rest.split('/').next().unwrap_or(rest))
}

pub struct Fetch;

#[derive(Default)]
struct Shared {
    pieces: VecDeque<Piece>,
    abort: Option<web_sys::AbortController>,
    cancelled: bool,
}

impl Transport for Fetch {
    fn send(&mut self, request: Request) -> Box<dyn Stream> {
        let shared = Rc::new(RefCell::new(Shared::default()));
        let o = origin(&request.url);
        let own = web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default();
        if o != own && !HOSTS.contains(&o.as_str()) {
            let host = o.split_once("://").map_or(o.as_str(), |(_, h)| h).to_string();
            shared.borrow_mut().pieces.push_back(Piece::Blocked(format!(
                "this page may reach only Anthropic, OpenAI, OpenRouter and local servers on ports 11434 (Ollama) and 1234 (LM Studio), not {host}; the desktop app can"
            )));
            return Box::new(FetchStream(shared));
        }
        let s = shared.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = run(request, &s).await {
                let mut s = s.borrow_mut();
                if !s.cancelled {
                    let msg =
                        e.as_string().or_else(|| Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string()));
                    let msg = msg.unwrap_or_else(|| "the request failed".into());
                    let msg = if msg.contains("Failed to fetch")
                        || msg.contains("NetworkError")
                        || msg.contains("Load failed")
                    {
                        format!("could not reach {o}: the network, or the server does not allow this page")
                    } else {
                        msg
                    };
                    s.pieces.push_back(Piece::Failed(msg));
                }
            }
        });
        Box::new(FetchStream(shared))
    }
}

async fn run(request: Request, s: &Rc<RefCell<Shared>>) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let ctrl = web_sys::AbortController::new()?;
    let headers = web_sys::Headers::new()?;
    for (k, v) in &request.headers {
        headers.set(k, v)?;
    }
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_headers(&headers);
    init.set_body(&JsValue::from_str(&request.body));
    init.set_signal(Some(&ctrl.signal()));
    init.set_referrer_policy(web_sys::ReferrerPolicy::NoReferrer);
    s.borrow_mut().abort = Some(ctrl);
    let url = request.url.clone();
    drop(request);
    let resp: web_sys::Response = JsFuture::from(window.fetch_with_str_and_init(&url, &init)).await?.dyn_into()?;
    s.borrow_mut().pieces.push_back(Piece::Status(resp.status()));
    let Some(body) = resp.body() else {
        s.borrow_mut().pieces.push_back(Piece::End);
        return Ok(());
    };
    let reader: web_sys::ReadableStreamDefaultReader = body.get_reader().dyn_into()?;
    loop {
        if s.borrow().cancelled {
            let _ = reader.cancel();
            return Ok(());
        }
        let r = JsFuture::from(reader.read()).await?;
        if Reflect::get(&r, &"done".into())?.as_bool().unwrap_or(true) {
            s.borrow_mut().pieces.push_back(Piece::End);
            return Ok(());
        }
        let chunk = Uint8Array::new(&Reflect::get(&r, &"value".into())?).to_vec();
        s.borrow_mut().pieces.push_back(Piece::Bytes(chunk));
    }
}

struct FetchStream(Rc<RefCell<Shared>>);

impl Stream for FetchStream {
    fn poll(&mut self) -> Vec<Piece> {
        let mut s = self.0.borrow_mut();
        if s.cancelled {
            return Vec::new();
        }
        s.pieces.drain(..).collect()
    }

    fn cancel(&mut self) {
        let mut s = self.0.borrow_mut();
        s.cancelled = true;
        s.pieces.clear();
        if let Some(c) = s.abort.take() {
            c.abort();
        }
    }
}

/// The keys this tab holds, and which the browser remembers.
#[derive(Default)]
pub struct KeyStore {
    keys: [Option<String>; 3],
    remembered: [bool; 3],
}

impl KeyStore {
    /// A key the page read back from the browser's encrypted store at start.
    pub fn restore(&mut self, kind: Kind, key: &str) {
        self.keys[kind.index()] = Some(key.into());
        self.remembered[kind.index()] = true;
    }
}

#[derive(Clone)]
pub struct WebKeys(pub Rc<RefCell<KeyStore>>);

/// Call `window.vxrKeys[name](args...)` and report a failure in the console.
fn vxr_keys(name: &str, args: &[JsValue]) {
    let call = || -> Result<JsValue, JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let keys = Reflect::get(&window, &"vxrKeys".into())?;
        let f: Function = Reflect::get(&keys, &name.into())?.dyn_into()?;
        let a = js_sys::Array::new();
        for v in args {
            a.push(v);
        }
        f.apply(&keys, &a)
    };
    match call() {
        Ok(p) => {
            if let Ok(p) = p.dyn_into::<js_sys::Promise>() {
                let name = name.to_string();
                wasm_bindgen_futures::spawn_local(async move {
                    if JsFuture::from(p).await.is_err() {
                        web_sys::console::warn_1(&format!("vxrKeys.{name} failed").into());
                    }
                });
            }
        }
        Err(_) => web_sys::console::warn_1(&format!("vxrKeys.{name} is missing").into()),
    }
}

impl Keys for WebKeys {
    fn get(&mut self, kind: Kind) -> Option<String> {
        self.0.borrow().keys[kind.index()].clone()
    }

    fn set(&mut self, kind: Kind, key: &str, remember: bool) -> Result<(), String> {
        let mut s = self.0.borrow_mut();
        s.keys[kind.index()] = Some(key.into());
        s.remembered[kind.index()] = remember;
        if remember {
            vxr_keys("remember", &[kind.id().into(), key.into()]);
        } else {
            vxr_keys("forget", &[kind.id().into()]);
        }
        Ok(())
    }

    fn remove(&mut self, kind: Kind) -> Result<(), String> {
        let mut s = self.0.borrow_mut();
        s.keys[kind.index()] = None;
        s.remembered[kind.index()] = false;
        vxr_keys("forget", &[kind.id().into()]);
        Ok(())
    }

    fn has(&mut self, kind: Kind) -> bool {
        self.0.borrow().keys[kind.index()].is_some()
    }

    fn place(&self) -> KeyPlace {
        KeyPlace::Browser
    }

    fn remembered(&self, kind: Kind) -> bool {
        self.0.borrow().remembered[kind.index()]
    }
}
