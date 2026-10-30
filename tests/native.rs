// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use day_part_webdav::*;
use futures_lite::future::block_on;

#[test]
#[ignore = "scripts/capture.py --native supplies an isolated loopback server and fresh root"]
fn native_resource_lifecycle() {
    lifecycle(http::Provider::Native, "native");
}

#[cfg(all(
    feature = "reqwest",
    not(target_arch = "wasm32"),
    not(target_env = "ohos")
))]
#[test]
#[ignore = "scripts/capture.py --native supplies an isolated server"]
fn reqwest_resource_lifecycle() {
    lifecycle(http::Provider::Reqwest, "reqwest");
}

fn lifecycle(provider: http::Provider, name: &str) {
    block_on(async {
        let session = http::Session::new();
        session.set_provider(provider);
        let endpoint =
            Endpoint::loopback(&std::env::var("DAV_FIXTURE_URL").expect("fixture URL")).unwrap();
        let client = Client::builder(endpoint)
            .session(session)
            .authentication(Authentication::Basic {
                username: "day_fixture".into(),
                password: std::env::var("DAV_FIXTURE_PASSWORD").unwrap(),
            })
            .build();
        assert!(!client.options().await.unwrap().dav.is_empty());
        let folder_name = format!("provider-{name}");
        let folder = ResourcePath::from_segments([folder_name.as_str()], true).unwrap();
        let file =
            ResourcePath::from_segments([folder_name.as_str(), "café + 100%.bin"], false).unwrap();
        let copy = ResourcePath::from_segments([folder_name.as_str(), "copy.bin"], false).unwrap();
        let moved =
            ResourcePath::from_segments([folder_name.as_str(), "moved.bin"], false).unwrap();
        client.mkcol(&folder).await.unwrap();
        let bytes = b"native fixture\x00\xff".to_vec();
        client
            .put(&file, bytes.clone(), Condition::CreateOnly)
            .await
            .unwrap();
        let listing = client.propfind(&folder, Depth::One).await.unwrap();
        assert_eq!(listing.resources.len(), 2);
        assert!(
            listing
                .resources
                .iter()
                .any(|r| r.content_length() == Some(bytes.len() as u64))
        );
        let fetched = client.get(&file, None).await.unwrap();
        assert_eq!(fetched.body, bytes);
        let etag = EntityTag::parse(fetched.header("etag").unwrap()).unwrap();
        assert_eq!(client.get(&file, Some(&etag)).await.unwrap().status, 304);
        let mut strong = etag;
        for _ in 0..40 {
            if !strong.is_weak() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            strong = EntityTag::parse(client.head(&file).await.unwrap().header("etag").unwrap())
                .unwrap();
        }
        assert!(
            matches!(client.get_range(&file,1,4,&strong).await.unwrap(),RangeDownload::Partial {response,..} if response.body==b"ativ")
        );
        assert!(matches!(
            client
                .get_range(&file, 1, 4, &EntityTag::parse("\"stale-fixture\"").unwrap())
                .await
                .unwrap(),
            RangeDownload::Restart(_)
        ));
        assert!(matches!(
            client
                .put(&file, b"replacement".to_vec(), Condition::CreateOnly)
                .await,
            Err(Error::Status { status: 412, .. })
        ));
        client.copy(&file, &copy, false).await.unwrap();
        client.move_resource(&copy, &moved, false).await.unwrap();
        assert_eq!(client.get(&moved, None).await.unwrap().body, bytes);
        client
            .delete(&moved, Condition::Unconditional)
            .await
            .unwrap();
        assert!(matches!(
            client.get(&moved, None).await,
            Err(Error::Status { status: 404, .. })
        ));
        client
            .delete(&folder, Condition::Unconditional)
            .await
            .unwrap();
    });
}
