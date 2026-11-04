//! The desktop app's sign-in against a running Worker, through the same [`Courier`] and
//! [`Library`] the window uses: the code from the email signs in and hands over the session
//! token, the token alone signs in again (a relaunch) and carries the library, and Sign out ends the session for both.
//! A request that says it is the app but comes from a page (it has an Origin) is refused.
//!
//! Ignored, as it needs the Worker up: `wrangler dev --local` with `EMAIL_SENDER=log`, its log
//! written to a file (see AGENTS.md), then
//! `VXR_SERVER=http://localhost:8787 VXR_WORKER_LOG=<log> ./dev cargo test -p vxr-app --test signin -- --ignored`.
//! Each run sends one email to a new address at example.com, which only the log sees; the
//! app's limit of five emails an hour from one IP holds here too.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use vxr_app::courier::Courier;
use vxr_app::projects::{Memory, Projects};
use vxr_app::sync::{Account, Auth, Event, Library};

/// Send every call `lib` queues, and those its answers queue, until none is left.
fn run(lib: &mut Library, projects: &mut Projects, courier: &Courier) -> Vec<Event> {
    let (mut events, mut waiting) = (Vec::new(), 0);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let calls = lib.take_out();
        waiting += calls.len();
        courier.send(calls);
        if waiting == 0 {
            return events;
        }
        for (id, reply) in courier.replies() {
            waiting -= 1;
            events.extend(lib.answer(id, reply, projects));
        }
        assert!(Instant::now() < deadline, "the Worker did not answer in 30 s");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The code in the last email to `to` the Worker printed to its log.
fn code_for(log: &str, to: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mark = format!("[vxr mail] to {to}: Sign in to vxr: ");
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if let Some(at) = text.rfind(&mark) {
            return text[at + mark.len()..].chars().take(6).collect();
        }
        assert!(Instant::now() < deadline, "no email to {to} in {log}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn email(lib: &Library) -> Option<String> {
    lib.account.user().and_then(|u| u.email.clone())
}

#[test]
#[ignore = "needs wrangler dev --local (see the module doc)"]
fn signs_in_by_code_and_out() {
    let server = std::env::var("VXR_SERVER").expect("VXR_SERVER: the local Worker's origin");
    let log = std::env::var("VXR_WORKER_LOG").expect("VXR_WORKER_LOG: the file the Worker logs to");
    assert!(server.starts_with("http://localhost:"), "only ever a Worker on this machine");
    let courier = Courier::new(&server, Arc::new(|| {}));
    let mut projects = Projects::load(Box::new(Memory::default()));
    let n = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or_default();
    let to = format!("app-{n}@example.com");

    // Posing as the app from a page: refused before any email.
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
    let posed = agent
        .post(&format!("{server}/api/auth/sign-in/magic-link"))
        .header("x-vxr-client", "app")
        .header("origin", &server)
        .header("content-type", "application/json")
        .send(format!(r#"{{"email":"{to}"}}"#).as_bytes())
        .map(|r| r.status().as_u16())
        .ok();
    assert_eq!(posed, Some(403), "the app's way in takes no request from a page");

    let mut app = Library::native(None);
    app.check();
    run(&mut app, &mut projects, &courier);
    assert_eq!(app.account, Account::Out);

    app.send_code(&to, None);
    let events = run(&mut app, &mut projects, &courier);
    assert!(events.contains(&Event::Auth(Auth::Sent)), "the email went out: {events:?}");
    let code = code_for(&log, &to);
    assert!(code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()), "a 6-digit code: {code:?}");

    app.verify_code(&to, &code);
    let events = run(&mut app, &mut projects, &courier);
    let token = events.iter().find_map(|e| match e {
        Event::Session(Some(t)) => Some(t.clone()),
        _ => None,
    });
    assert!(token.is_some(), "the Worker handed over the session token: {events:?}");
    assert_eq!(email(&app).as_deref(), Some(to.as_str()));

    // A relaunch with the kept token.
    let mut again = Library::native(token);
    again.check();
    run(&mut again, &mut projects, &courier);
    assert_eq!(email(&again).as_deref(), Some(to.as_str()));

    // The token carries the library too: a save goes up and is listed.
    let id = projects.opened("cube.vox", None, None, None, false, 0);
    let mut cube = vxr_core::Scene::new([8, 8, 8]);
    cube.edit([([0, 0, 0], 3u8)]);
    app.upload(&mut projects, &id, cube.to_vox().expect("a .vox"), None);
    run(&mut app, &mut projects, &courier);
    let user = app.account.user().map(|u| u.id.clone()).unwrap_or_default();
    let cloud = projects.get(&id).and_then(|p| p.library(&user)).cloned();
    assert!(cloud.as_ref().is_some_and(|c| c.version.is_some() && !c.dirty), "uploaded: {cloud:?}");
    again.list();
    run(&mut again, &mut projects, &courier);
    let theirs = projects.get(&id).and_then(|p| p.cloud.as_ref()).and_then(|c| c.latest.clone());
    assert_eq!(theirs, cloud.and_then(|c| c.version), "the relaunched app lists it");

    app.sign_out();
    let events = run(&mut app, &mut projects, &courier);
    assert!(events.contains(&Event::Session(None)), "signing out deletes the token: {events:?}");
    assert_eq!(app.account, Account::Out);

    again.check();
    let events = run(&mut again, &mut projects, &courier);
    assert!(events.contains(&Event::Session(None)), "the ended session's token is deleted there too: {events:?}");
    assert_eq!(again.account, Account::Out);
}
