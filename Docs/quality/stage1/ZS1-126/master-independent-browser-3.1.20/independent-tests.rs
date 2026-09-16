use super::*;
use crate::session::{SessionStore, SessionSummary};
use std::sync::mpsc::{self, Receiver, SyncSender};

enum Answer {
    Row(&'static str),
    Failure(&'static str),
    Close,
}
struct BarrierProbe {
    host: SessionBrowserHost,
    entered: Receiver<(SessionBrowserKey, bool)>,
    release: SyncSender<Answer>,
}
fn row(name: &str) -> SessionSummary {
    SessionSummary {
        path: name.into(),
        session_id: name.into(),
        created_at_ms: 0,
        turn_count: 0,
        handoff_count: 0,
        handoff_record_count: 0,
        runtime_intent_count: 0,
        event_count: 0,
        recovery_warnings: 0,
        next_seq: 0,
    }
}
fn probe() -> BarrierProbe {
    let (started, entered) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let host = SessionBrowserHost::with_scanner(move |key, explicit| {
        started.send((key.clone(), explicit)).unwrap();
        match wait
            .recv_timeout(Duration::from_secs(3))
            .expect("test barrier release")
        {
            Answer::Row(name) => SessionBrowserProjection {
                rows: Ok(vec![row(name)]),
                receipt: explicit.then(|| Ok(name.into())),
            },
            Answer::Failure(name) => SessionBrowserProjection {
                rows: Err(name.into()),
                receipt: explicit.then(|| Err(name.into())),
            },
            Answer::Close => panic!("intentional independent scanner closure"),
        }
    })
    .unwrap();
    BarrierProbe {
        host,
        entered,
        release,
    }
}
fn state(owner: &SessionStore) -> TuiState {
    let mut state = TuiState {
        async_session_browser: true,
        ..TuiState::default()
    };
    state.refresh_session_snapshot(owner);
    state
}
fn search(p: &mut BarrierProbe, s: &mut TuiState, query: &str) {
    assert!(p.host.route(
        &SlashCommand::Session {
            action: crate::slash::SessionAction::Search {
                query: query.into()
            }
        },
        s,
        format!("/session search {query}")
    ));
}
fn next(p: &BarrierProbe) -> (SessionBrowserKey, bool) {
    p.entered
        .recv_timeout(Duration::from_secs(2))
        .expect("worker must enter")
}
fn until(p: &mut BarrierProbe, s: &mut TuiState, condition: impl Fn(&SessionBrowserHost) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        p.host.poll(s);
        if condition(&p.host) {
            break;
        }
        assert!(Instant::now() < deadline, "bounded poll did not settle");
        std::thread::yield_now();
    }
}
fn complete(p: &mut BarrierProbe, s: &mut TuiState, answer: Answer) {
    p.release.send(answer).unwrap();
    until(p, s, |h| h.active.is_none() && h.pending.is_none());
}

#[test]
fn independent_same_query_generation_and_latest_pending_ignore_old_failure() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SessionStore::open(dir.path().join("owner.jsonl")).unwrap();
    let mut s = state(&owner);
    let mut p = probe();
    search(&mut p, &mut s, "same");
    p.host.poll(&mut s);
    let old = next(&p).0;
    for q in ["one", "two", "three", "same"] {
        search(&mut p, &mut s, q);
        p.host.poll(&mut s);
    }
    let latest = p.host.pending.as_ref().unwrap().key.clone();
    assert_eq!(old.query, latest.query);
    assert_eq!(latest.generation, old.generation + 4);
    assert!(p.entered.try_recv().is_err());
    p.release.send(Answer::Failure("OBSOLETE-FAILURE")).unwrap();
    until(&mut p, &mut s, |h| {
        h.active.as_ref().is_some_and(|r| r.key == latest)
    });
    assert_eq!(next(&p).0, latest);
    assert!(!s.messages().any(|m| m.text.contains("OBSOLETE")));
    assert!(s.input().is_empty());
    assert!(s.session_browser.is_empty());
    complete(&mut p, &mut s, Answer::Row("latest-only"));
    assert_eq!(s.session_browser[0].path, "latest-only");
    assert_eq!(s.session_browser_refresh.query.as_deref(), Some("same"));
}

#[test]
fn independent_observed_project_roundtrip_restores_draft_and_rejects_old_a() {
    let dir = tempfile::tempdir().unwrap();
    let a = SessionStore::open(dir.path().join("a.jsonl")).unwrap();
    let b = SessionStore::open(dir.path().join("b.jsonl")).unwrap();
    let mut s = state(&a);
    s.session_browser_refresh.query = Some("saved-A-filter".into());
    s.session_browser = vec![row("saved-A-selection")];
    let mut p = probe();
    search(&mut p, &mut s, "in-flight-A");
    p.host.poll(&mut s);
    let old = next(&p).0;
    s.set_input("A draft");
    assert!(s.open_project_tab("B"));
    s.refresh_session_snapshot(&b);
    s.set_input("B draft");
    p.host.poll(&mut s);
    assert!(s.select_project_tab(0));
    s.refresh_session_snapshot(&a);
    p.host.poll(&mut s);
    assert_eq!(s.input(), "A draft");
    assert_eq!(
        s.session_browser_refresh.query.as_deref(),
        Some("saved-A-filter")
    );
    assert_eq!(s.selected_session_browser_path(), Some("saved-A-selection"));
    let latest = p.host.pending.as_ref().unwrap().key.clone();
    assert_eq!(latest.scope, old.scope);
    assert!(latest.generation > old.generation);
    p.release.send(Answer::Failure("OLD-A-FAILURE")).unwrap();
    until(&mut p, &mut s, |h| {
        h.active.as_ref().is_some_and(|r| r.key == latest)
    });
    let (key, explicit) = next(&p);
    assert_eq!(key, latest);
    assert!(!explicit);
    assert_eq!(s.input(), "A draft");
    assert!(!s.messages().any(|m| m.text.contains("OLD-A")));
    complete(&mut p, &mut s, Answer::Row("fresh-A"));
    assert!(s.select_project_tab(1));
    assert_eq!(s.input(), "B draft");
}

#[test]
fn independent_same_directory_path_or_session_id_change_rejects_old_reply() {
    for dimension in ["path", "id"] {
        let dir = tempfile::tempdir().unwrap();
        let owner = SessionStore::open(dir.path().join("one.jsonl")).unwrap();
        let mut s = state(&owner);
        let mut p = probe();
        search(&mut p, &mut s, "q");
        p.host.poll(&mut s);
        let old = next(&p).0;
        let tuple = s.session_browser_refresh.owner.as_mut().unwrap();
        if dimension == "path" {
            tuple.0 = dir.path().join("two.jsonl");
        } else {
            tuple.1 = "new-identity-same-path".into();
        }
        p.host.poll(&mut s);
        let desired = p.host.pending.as_ref().unwrap().key.clone();
        assert_eq!(old.scope.directory, desired.scope.directory);
        assert_ne!(old.scope, desired.scope);
        p.release.send(Answer::Row("stale-scope")).unwrap();
        until(&mut p, &mut s, |h| {
            h.active.as_ref().is_some_and(|r| r.key == desired)
        });
        next(&p);
        assert!(s.session_browser.is_empty());
        assert!(!s.messages().any(|m| m.text.contains("stale-scope")));
        complete(&mut p, &mut s, Answer::Row("current-scope"));
        assert_eq!(s.session_browser[0].path, "current-scope");
    }
}

#[test]
fn independent_active_failure_preserves_new_visible_or_held_character_draft() {
    for held in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let owner = SessionStore::open(dir.path().join("owner.jsonl")).unwrap();
        let mut s = state(&owner);
        s.session_browser = vec![row("old-pane")];
        s.session_browser_refresh.query = Some("old-filter".into());
        let mut p = probe();
        search(&mut p, &mut s, "fails");
        p.host.poll(&mut s);
        next(&p);
        if held {
            s.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        } else {
            s.set_input("new draft");
        }
        complete(&mut p, &mut s, Answer::Failure("expected-failure"));
        s.finish_ordinary_paste();
        assert_eq!(s.input(), if held { "n" } else { "new draft" });
        assert_eq!(s.session_browser[0].path, "old-pane");
        assert_eq!(
            s.session_browser_refresh.query.as_deref(),
            Some("old-filter")
        );
    }
}

#[test]
fn independent_worker_closure_restores_only_latest_pending_and_rejects_future_commands() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SessionStore::open(dir.path().join("owner.jsonl")).unwrap();
    let mut s = state(&owner);
    let mut p = probe();
    search(&mut p, &mut s, "old");
    p.host.poll(&mut s);
    next(&p);
    search(&mut p, &mut s, "middle");
    search(&mut p, &mut s, "latest");
    complete(&mut p, &mut s, Answer::Close);
    assert!(p.host.closed);
    assert_eq!(s.input(), "/session search latest");
    s.set_input("new draft");
    let generation = p.host.generation;
    search(&mut p, &mut s, "rejected");
    assert_eq!(p.host.generation, generation);
    assert_eq!(s.input(), "new draft");
    for _ in 0..5 {
        assert!(!p.host.poll(&mut s));
    }
    assert!(p.host.active.is_none() && p.host.pending.is_none());
}

#[test]
fn independent_no_owner_overflow_and_periodic_refresh_are_bounded() {
    let mut s = TuiState {
        async_session_browser: true,
        ..TuiState::default()
    };
    let mut p = probe();
    for _ in 0..20 {
        p.host.poll(&mut s);
    }
    assert!(p.host.active.is_none() && p.host.pending.is_none());
    assert_eq!(p.host.generation, 0);
    search(&mut p, &mut s, "no-owner");
    assert_eq!(s.input(), "/session search no-owner");
    assert_eq!(p.host.generation, 0);
    let dir = tempfile::tempdir().unwrap();
    let owner = SessionStore::open(dir.path().join("owner.jsonl")).unwrap();
    s.refresh_session_snapshot(&owner);
    s.set_input("");
    p.host.generation = u64::MAX;
    search(&mut p, &mut s, "overflow");
    assert_eq!(s.input(), "/session search overflow");
    assert_eq!(p.host.generation, u64::MAX);
    assert!(p.host.pending.is_none());
    for _ in 0..20 {
        p.host.poll(&mut s);
    }
    assert!(p.entered.try_recv().is_err());
    drop(p);
    let mut p = probe();
    s.set_input("");
    p.host.poll(&mut s);
    assert!(!next(&p).1);
    complete(&mut p, &mut s, Answer::Row("initial"));
    s.session_browser_refresh.last_checked = Some(Instant::now());
    for _ in 0..10 {
        p.host.poll(&mut s);
    }
    assert!(p.host.active.is_none());
    s.session_browser_refresh.last_checked = Some(Instant::now() - Duration::from_secs(1));
    p.host.poll(&mut s);
    assert!(!next(&p).1);
    complete(&mut p, &mut s, Answer::Row("periodic"));
    assert_eq!(s.session_browser[0].path, "periodic");
}

#[test]
fn independent_unread_reply_drop_disconnects_before_join() {
    let dir = tempfile::tempdir().unwrap();
    let owner = SessionStore::open(dir.path().join("owner.jsonl")).unwrap();
    let mut s = state(&owner);
    let mut p = probe();
    p.host.poll(&mut s);
    next(&p);
    p.release.send(Answer::Row("unread")).unwrap();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let join = std::thread::spawn(move || {
        drop(p);
        done_tx.send(()).unwrap();
    });
    done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("drop must disconnect result channel before join");
    join.join().unwrap();
}

#[test]
fn independent_local_pane_and_global_receipt_remain_separate() {
    let local = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    let owner = SessionStore::open(local.path().join("local.jsonl")).unwrap();
    let global_owner = SessionStore::open(global.path().join("global.jsonl")).unwrap();
    let global_id = global_owner.session_id().to_owned();
    let path = global.path().to_owned();
    let mut s = state(&owner);
    s.session_browser_refresh.query = Some("old-filter".into());
    let mut host = SessionBrowserHost::with_scanner(move |key, explicit| {
        let rows = crate::session::list_sessions(&key.scope.directory).map_err(|e| e.to_string());
        let receipt = explicit.then(|| {
            crate::session::list_sessions(&path)
                .map(|v| {
                    v.into_iter()
                        .map(|r| r.session_id)
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .map_err(|e| e.to_string())
        });
        SessionBrowserProjection { rows, receipt }
    })
    .unwrap();
    assert!(host.route(
        &SlashCommand::Session {
            action: crate::slash::SessionAction::List
        },
        &mut s,
        "/session list".into()
    ));
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        host.poll(&mut s);
        if host.active.is_none() && host.pending.is_none() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(s.session_browser_refresh.query.is_none());
    assert_eq!(s.session_browser.len(), 1);
    assert_eq!(s.session_browser[0].session_id, owner.session_id());
    assert!(s.messages().any(|m| m.text.contains(&global_id)));
    assert!(!s.messages().any(|m| m.text.contains(owner.session_id())));
}
