// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! All fixture bodies are synthetic server exchanges, not bundled application resources.
use base64::{Engine, engine::general_purpose::STANDARD};
use day_part_webdav::{
    http::{
        simulation::{Reply, Simulation},
        transport::PreparedBody,
    },
    *,
};
use futures_lite::future::block_on;
use serde::Deserialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

#[derive(Deserialize)]
struct Fixture {
    base_url: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    label: String,
    method: String,
    relative: String,
    request_headers: HashMap<String, String>,
    request_body: String,
    status: u16,
    response_headers: Vec<(String, String)>,
    body: String,
}
fn replay(raw: &str) {
    let fixture: Fixture = serde_json::from_str(raw).unwrap();
    let range_tag = EntityTag::parse(
        &fixture
            .cases
            .iter()
            .find(|c| c.label == "range")
            .unwrap()
            .request_headers["If-Range"],
    )
    .unwrap();
    let base = fixture.base_url.clone();
    let queue = Arc::new(Mutex::new(VecDeque::from(fixture.cases)));
    let pending = queue.clone();
    let simulation = Simulation::new();
    simulation.handle(
        |_, _| true,
        move |request| {
            let case = pending
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request");
            assert_eq!(request.method, case.method, "{}", case.label);
            assert_eq!(
                request.url,
                format!("{base}{}", case.relative),
                "{}",
                case.label
            );
            for (name, value) in case.request_headers {
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|(k, v)| k.eq_ignore_ascii_case(&name) && v == &value),
                    "{} missing {name}",
                    case.label
                );
            }
            let body = match request.body {
                PreparedBody::Empty => vec![],
                PreparedBody::Bytes(b) => b.as_ref().clone(),
                _ => panic!("expected bounded byte payload"),
            };
            assert_eq!(
                body,
                STANDARD.decode(case.request_body).unwrap(),
                "{}",
                case.label
            );
            let mut reply = Reply::new(case.status, STANDARD.decode(case.body).unwrap());
            reply.headers = case.response_headers;
            Ok(reply)
        },
    );
    let session = http::Session::new();
    session.set_simulation(Some(simulation.clone()));
    let client = Client::builder(Endpoint::loopback(&fixture.base_url).unwrap())
        .session(session)
        .build();
    block_on(async {
        client.options().await.unwrap();
        let dir = ResourcePath::from_segments(["items"], true).unwrap();
        let file = ResourcePath::from_segments(["items", "café + 100%.bin"], false).unwrap();
        let copy = ResourcePath::from_segments(["items", "copy.bin"], false).unwrap();
        let moved = ResourcePath::from_segments(["items", "moved.bin"], false).unwrap();
        client.mkcol(&dir).await.unwrap();
        client
            .put(
                &file,
                b"synthetic dav payload\x00\xff".to_vec(),
                Condition::CreateOnly,
            )
            .await
            .unwrap();
        let listing = client.propfind(&dir, Depth::One).await.unwrap();
        assert_eq!(listing.resources.len(), 2);
        assert!(listing.resources.iter().any(Resource::is_collection));
        let get = client.get(&file, None).await.unwrap();
        assert_eq!(get.body, b"synthetic dav payload\x00\xff");
        client.head(&file).await.unwrap();
        let etag = EntityTag::parse(get.header("etag").unwrap()).unwrap();
        assert_eq!(client.get(&file, Some(&etag)).await.unwrap().status, 304);
        assert!(matches!(
            client
                .put(&file, b"other".to_vec(), Condition::CreateOnly)
                .await,
            Err(Error::Status { status: 412, .. })
        ));
        assert!(matches!(
            client
                .put(
                    &file,
                    b"other".to_vec(),
                    Condition::IfMatch(EntityTag::parse("\"stale-fixture\"").unwrap())
                )
                .await,
            Err(Error::Status { status: 412, .. })
        ));
        assert!(matches!(
            client
                .mkcol(&ResourcePath::from_segments(["missing", "child"], true).unwrap())
                .await,
            Err(Error::Status { status: 409, .. })
        ));
        client.copy(&file, &copy, false).await.unwrap();
        assert!(matches!(
            client.copy(&file, &copy, false).await,
            Err(Error::Status { status: 412, .. })
        ));
        client.move_resource(&copy, &moved, false).await.unwrap();
        client
            .delete(&moved, Condition::Unconditional)
            .await
            .unwrap();
        assert!(matches!(
            client
                .get(
                    &ResourcePath::from_segments(["items", "missing.bin"], false).unwrap(),
                    None
                )
                .await,
            Err(Error::Status { status: 404, .. })
        ));
        assert_eq!(
            client
                .propfind(&file, Depth::Zero)
                .await
                .unwrap()
                .resources
                .len(),
            1
        );
        assert!(
            matches!(client.get_range(&file,1,4,&range_tag).await.unwrap(),RangeDownload::Partial {response,..} if response.body==b"ynth")
        );
        assert!(matches!(
            client
                .get_range(&file, 1, 4, &EntityTag::parse("\"stale-fixture\"").unwrap())
                .await
                .unwrap(),
            RangeDownload::Restart(_)
        ));
    });
    assert!(queue.lock().unwrap().is_empty());
    assert_eq!(simulation.requests().len(), 18);
}
#[test]
fn replay_apache() {
    replay(include_str!("fixtures/apache.json"));
}
#[test]
fn replay_nextcloud() {
    replay(include_str!("fixtures/nextcloud.json"));
}

#[test]
fn apache_verbose_listing_supports_sync_history_and_reports_node_exhaustion() {
    // Expand a real synthetic Apache response, preserving its namespaces, whitespace, and
    // per-property 404 statuses. The former 100k node budget failed before 4096 objects.
    let f: Fixture = serde_json::from_str(include_str!("fixtures/apache.json")).unwrap();
    let c = f.cases.iter().find(|c| c.label == "propfind").unwrap();
    let xml = String::from_utf8(STANDARD.decode(&c.body).unwrap()).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let response = doc
        .root_element()
        .children()
        .rfind(|n| n.has_tag_name(("DAV:", "response")))
        .unwrap();
    let href = response
        .children()
        .find(|n| n.has_tag_name(("DAV:", "href")))
        .unwrap()
        .text()
        .unwrap();
    let template = &xml[response.range()];
    let endpoint = Endpoint::loopback(&f.base_url).unwrap();
    let requested = format!("{}items/", f.base_url);
    let path = href.rsplit_once('/').unwrap().0;
    let mut large = String::from("<D:multistatus xmlns:D='DAV:' xmlns:ns0='DAV:'>");
    for i in 0..4130 {
        large.push_str(&template.replace(href, &format!("{path}/synthetic-{i}")));
    }
    large.push_str("</D:multistatus>");
    let limits = XmlLimits::default();
    assert_eq!(
        MultiStatus::parse(large.as_bytes(), &endpoint, &requested, &limits)
            .unwrap()
            .resources
            .len(),
        4130
    );
    assert!(matches!(
        MultiStatus::parse(
            large.as_bytes(),
            &endpoint,
            &requested,
            &XmlLimits {
                nodes: 100_000,
                ..limits
            }
        ),
        Err(Error::Limit)
    ));
}
