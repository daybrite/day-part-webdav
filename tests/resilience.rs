// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use day_part_webdav::{
    http::simulation::{Conditions, ManualClock, Reply, Simulation},
    *,
};
use futures_lite::future::block_on;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}
fn setup(policy: Policy) -> (Client, Simulation, ManualClock) {
    let clock = ManualClock::default();
    let simulation = Simulation::with_clock(Arc::new(clock.clone()));
    let session = http::Session::new();
    session.set_simulation(Some(simulation.clone()));
    let client = Client::builder(Endpoint::new("https://fixture.invalid/dav/").unwrap())
        .session(session)
        .policy(policy)
        .clock(Arc::new(clock.clone()))
        .build();
    (client, simulation, clock)
}
fn drive<F: Future>(future: F, clock: &ManualClock) -> F::Output {
    let mut future = Box::pin(future);
    for _ in 0..1000 {
        if let Poll::Ready(value) = poll(future.as_mut()) {
            return value;
        }
        clock.advance(Duration::from_millis(100));
    }
    panic!("future did not finish within deterministic budget")
}
#[test]
fn interrupted_read_retries_without_returning_partial_bytes() {
    let (client, sim, clock) = setup(Policy::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let n = calls.clone();
    sim.handle(
        |_, _| true,
        move |_| {
            let mut r = Reply::new(200, b"complete".to_vec());
            if n.fetch_add(1, Ordering::SeqCst) == 0 {
                r.chunks = vec![b"partial".to_vec()];
                r.fail_after_chunks = Some(1);
            }
            Ok(r)
        },
    );
    let result = drive(client.get(&ResourcePath::root(), None), &clock).unwrap();
    assert_eq!(result.body, b"complete");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[test]
fn ambiguous_write_is_not_replayed() {
    let (client, sim, clock) = setup(Policy::default());
    sim.handle(|_, _| true, |_| Err(http::HttpError::Connect));
    let result = drive(
        client.put(
            &ResourcePath::root(),
            b"intent".to_vec(),
            Condition::CreateOnly,
        ),
        &clock,
    );
    assert!(matches!(
        result,
        Err(Error::Transport {
            outcome: WriteOutcome::MayHaveApplied,
            ..
        })
    ));
    assert_eq!(sim.requests().len(), 1);
}

#[test]
fn resumed_bytes_require_consistent_range_representation_and_validator() {
    for (range, etag, encoding, body) in [
        ("bytes 1-3/10", "\"v1\"", "identity", b"abc".as_slice()),
        ("bytes 0-2/10", "\"v1\"", "identity", b"abc".as_slice()),
        ("bytes 1-4/10", "\"v2\"", "identity", b"abcd".as_slice()),
        ("bytes 1-4/10", "\"v1\"", "gzip", b"abcd".as_slice()),
        ("bytes 1-4/4", "\"v1\"", "identity", b"abcd".as_slice()),
        ("bytes 1-4/*", "\"v1\"", "identity", b"abcd".as_slice()),
        ("bytes 1-4/10", "\"v1\"", "identity", b"abc".as_slice()),
    ] {
        let (client, sim, clock) = setup(Policy::default());
        sim.handle(
            |_, _| true,
            move |_| {
                Ok(Reply::new(206, body.to_vec())
                    .header("Content-Range", range)
                    .header("ETag", etag)
                    .header("Content-Encoding", encoding))
            },
        );
        let result = drive(
            client.get_range(
                &ResourcePath::root(),
                1,
                4,
                &EntityTag::parse("\"v1\"").unwrap(),
            ),
            &clock,
        );
        assert!(
            matches!(result, Err(Error::Protocol)),
            "{range}, {etag}, {encoding}"
        );
        assert_eq!(sim.requests().len(), 1);
    }
}
#[test]
fn retry_after_is_respected_without_sleeping_a_thread() {
    let (client, sim, clock) = setup(Policy::default());
    let n = Arc::new(AtomicUsize::new(0));
    let count = n.clone();
    sim.handle(
        |_, _| true,
        move |_| {
            Ok(if count.fetch_add(1, Ordering::SeqCst) == 0 {
                Reply::new(429, vec![]).header("Retry-After", "2")
            } else {
                Reply::new(200, b"ok".to_vec())
            })
        },
    );
    let root = ResourcePath::root();
    let mut f = Box::pin(client.get(&root, None));
    assert!(poll(f.as_mut()).is_pending());
    clock.advance(Duration::ZERO);
    assert!(poll(f.as_mut()).is_pending());
    clock.advance(Duration::from_millis(1900));
    assert!(poll(f.as_mut()).is_pending());
    assert_eq!(n.load(Ordering::SeqCst), 1);
    let result = drive(f, &clock).unwrap();
    assert_eq!(result.body, b"ok");
    assert_eq!(n.load(Ordering::SeqCst), 2);
}
#[test]
fn long_retry_after_goes_back_to_scheduler() {
    let (client, sim, clock) = setup(Policy::default());
    sim.handle(
        |_, _| true,
        |_| Ok(Reply::new(503, vec![]).header("Retry-After", "3600")),
    );
    assert!(
        matches!(drive(client.get(&ResourcePath::root(),None),&clock),Err(Error::Status {status:503,retry_after:Some(d),..}) if d==Duration::from_secs(3600))
    );
    assert_eq!(sim.requests().len(), 1);
}
#[test]
fn cancel_during_backoff_never_starts_another_request() {
    let (client, sim, clock) = setup(Policy::default());
    sim.handle(|_, _| true, |_| Ok(Reply::new(503, vec![])));
    let root = ResourcePath::root();
    let mut f = Box::pin(client.get(&root, None));
    assert!(poll(f.as_mut()).is_pending());
    clock.advance(Duration::ZERO);
    assert!(poll(f.as_mut()).is_pending());
    drop(f);
    clock.advance(Duration::from_secs(30));
    assert_eq!(sim.requests().len(), 1);
}
#[test]
fn constrained_expensive_offline_admission_uses_no_network() {
    let (client, sim, _) = setup(Policy::default());
    for state in [
        NetworkState::Constrained,
        NetworkState::Expensive,
        NetworkState::Offline,
    ] {
        client.set_network_state(state);
        assert!(
            matches!(block_on(client.get(&ResourcePath::root(),None)),Err(Error::Deferred(s)) if s==state)
        );
    }
    assert!(sim.requests().is_empty());
}
#[test]
fn network_policy_is_rechecked_after_backoff() {
    let (client, sim, clock) = setup(Policy::default());
    sim.handle(|_, _| true, |_| Ok(Reply::new(503, vec![])));
    let root = ResourcePath::root();
    let mut f = Box::pin(client.get(&root, None));
    assert!(poll(f.as_mut()).is_pending());
    clock.advance(Duration::ZERO);
    assert!(poll(f.as_mut()).is_pending());
    client.set_network_state(NetworkState::Constrained);
    assert!(matches!(
        drive(f, &clock),
        Err(Error::Deferred(NetworkState::Constrained))
    ));
    assert_eq!(sim.requests().len(), 1);
}
#[test]
fn retry_budget_stops_persistent_failures() {
    let (client, sim, clock) = setup(Policy::default());
    sim.handle(|_, _| true, |_| Err(http::HttpError::Connect));
    assert!(drive(client.get(&ResourcePath::root(), None), &clock).is_err());
    assert_eq!(sim.requests().len(), 3);
}
#[test]
fn permanent_statuses_and_tls_failures_are_not_retried() {
    for status in [401, 403, 404, 409, 412, 413, 507] {
        let (client, sim, clock) = setup(Policy::default());
        sim.handle(|_, _| true, move |_| Ok(Reply::new(status, vec![])));
        assert!(
            matches!(drive(client.get(&ResourcePath::root(),None),&clock),Err(Error::Status {status:s,..}) if s==status)
        );
        assert_eq!(sim.requests().len(), 1);
    }
    let (client, sim, clock) = setup(Policy::default());
    sim.handle(|_, _| true, |_| Err(http::HttpError::Tls("fixture".into())));
    assert!(drive(client.get(&ResourcePath::root(), None), &clock).is_err());
    assert_eq!(sim.requests().len(), 1);
}
#[test]
fn response_and_upload_limits_fail_closed() {
    let p = Policy {
        max_response_bytes: 4,
        max_upload_bytes: 4,
        ..Default::default()
    };
    let (client, sim, clock) = setup(p);
    sim.handle(|_, _| true, |_| Ok(Reply::new(200, b"12345".to_vec())));
    assert!(matches!(
        drive(client.get(&ResourcePath::root(), None), &clock),
        Err(Error::Transport {
            source: http::HttpError::BodyTooLarge { .. },
            ..
        })
    ));
    assert!(matches!(
        block_on(client.put(&ResourcePath::root(), vec![0; 5], Condition::CreateOnly)),
        Err(Error::Limit)
    ));
    assert_eq!(sim.requests().len(), 1);
}
#[test]
fn redirects_do_not_forward_credentials_or_mutate_methods() {
    let (client, sim, clock) = setup(Policy::default());
    sim.handle(
        |_, _| true,
        |_| Ok(Reply::new(302, vec![]).header("Location", "https://evil.invalid/login")),
    );
    assert!(matches!(
        drive(client.propfind(&ResourcePath::root(), Depth::One), &clock),
        Err(Error::Redirect { status: 302, .. })
    ));
    assert_eq!(sim.requests().len(), 1);
    assert_eq!(sim.requests()[0].method, "PROPFIND");
}
#[test]
fn partial_multi_status_delete_is_not_success() {
    let (client, sim, clock) = setup(Policy::default());
    sim.handle(|_,_|true,|_|Ok(Reply::new(207,br#"<multistatus xmlns="DAV:"><response><href>/dav/locked</href><status>HTTP/1.1 423 Locked</status></response></multistatus>"#.to_vec())));
    assert!(
        matches!(drive(client.delete(&ResourcePath::root(),Condition::Unconditional),&clock),Err(Error::Partial(m)) if m.has_failures())
    );
    assert_eq!(sim.requests().len(), 1);
}
#[test]
fn weak_etag_write_fails_before_network() {
    let (client, sim, _) = setup(Policy::default());
    assert!(matches!(
        block_on(client.put(
            &ResourcePath::root(),
            vec![],
            Condition::IfMatch(EntityTag::parse("W/\"a\"").unwrap())
        )),
        Err(Error::InvalidEntityTag)
    ));
    assert!(sim.requests().is_empty());
}
#[test]
fn slow_link_obeys_total_deadline() {
    let p = Policy {
        request_timeout: Duration::from_millis(400),
        retry: Retry {
            max_attempts: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let (client, sim, clock) = setup(p);
    sim.set_conditions(Conditions {
        bytes_per_second: Some(1),
        ..Default::default()
    });
    sim.handle(|_, _| true, |_| Ok(Reply::new(200, vec![0; 20])));
    assert!(matches!(
        drive(client.get(&ResourcePath::root(), None), &clock),
        Err(Error::Transport {
            source: http::HttpError::Timeout,
            ..
        })
    ));
    assert_eq!(sim.requests().len(), 1);
}
