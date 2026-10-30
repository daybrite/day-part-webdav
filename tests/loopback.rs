// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
#![cfg(all(feature = "demo-server", not(target_arch = "wasm32")))]
use day_part_webdav::*;

#[test]
fn native_demo_fixture() {
    exercise(http::Provider::Native);
}

#[cfg(all(feature = "reqwest", not(target_env = "ohos")))]
#[test]
fn reqwest_demo_fixture() {
    exercise(http::Provider::Reqwest);
}

fn exercise(provider: http::Provider) {
    let server = testing::Server::start().unwrap();
    let session = http::Session::new();
    session.set_provider(provider);
    let client = Client::builder(Endpoint::loopback(&server.url()).unwrap())
        .session(session)
        .build();
    futures_lite::future::block_on(async {
        assert!(client.options().await.unwrap().dav.contains(&"1".into()));
        let file = ResourcePath::from_segments(["café + 100%.bin"], false).unwrap();
        let body = b"fixture bytes\x00\xff".to_vec();
        client
            .put(&file, body.clone(), Condition::CreateOnly)
            .await
            .unwrap();
        assert_eq!(client.get(&file, None).await.unwrap().body, body);
        let response = client.head(&file).await.unwrap();
        let tag = EntityTag::parse(response.header("etag").unwrap()).unwrap();
        assert_eq!(client.get(&file, Some(&tag)).await.unwrap().status, 304);
        assert!(matches!(client.get_range(&file, 1, 4, &tag).await.unwrap(),
            RangeDownload::Partial { response, .. } if response.body == b"ixtu"));
        assert!(matches!(
            client.put(&file, vec![], Condition::CreateOnly).await,
            Err(Error::Status { status: 412, .. })
        ));
        assert_eq!(
            client
                .propfind(&ResourcePath::root(), Depth::One)
                .await
                .unwrap()
                .resources
                .len(),
            3
        );
        client.delete(&file, Condition::IfMatch(tag)).await.unwrap();
        assert!(matches!(
            client.get(&file, None).await,
            Err(Error::Status { status: 404, .. })
        ));
        let interrupted = ResourcePath::from_segments(["__drop__"], false).unwrap();
        assert!(
            matches!(
                client.get(&interrupted, None).await,
                Err(Error::Transport { .. })
            ),
            "a truncated Content-Length must never return successful partial bytes"
        );
    });
}
