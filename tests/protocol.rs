// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use day_part_webdav::*;

fn parse(xml: &str) -> Result<MultiStatus, Error> {
    MultiStatus::parse(
        xml.as_bytes(),
        &Endpoint::new("https://fixture.invalid/dav/").unwrap(),
        "https://fixture.invalid/dav/",
        &XmlLimits::default(),
    )
}
#[test]
fn namespace_and_property_status_are_authoritative() {
    let doc=parse(r#"<multistatus xmlns="DAV:" xmlns:x="urn:vendor"><response><href>/dav/a%20b</href><propstat><prop><getetag>W/"a"</getetag><getcontentlength>9</getcontentlength><x:extra>kept</x:extra></prop><status>HTTP/1.1 200 OK</status></propstat><propstat><prop><displayname/></prop><status>HTTP/1.1 404 Not Found</status></propstat></response></multistatus>"#).unwrap();
    let r = &doc.resources[0];
    assert_eq!(r.etag(), Some("W/\"a\""));
    assert_eq!(r.content_length(), Some(9));
    assert!(r.property("DAV:", "displayname").is_none());
    assert_eq!(r.property("urn:vendor", "extra").unwrap().text, "kept");
    assert!(doc.has_failures());
}
#[test]
fn hostile_or_ambiguous_xml_is_rejected() {
    for xml in [
        "<html>login</html>",
        "<multistatus/>",
        r#"<!DOCTYPE x [<!ENTITY a "bad">]><multistatus xmlns="DAV:">&a;</multistatus>"#,
        r#"<multistatus xmlns="DAV:"><response><href>/dav/a</href></response></multistatus>"#,
        r#"<multistatus xmlns="DAV:"><response><href>/dav/a</href><status>200</status></response></multistatus>"#,
        r#"<multistatus xmlns="DAV:"><response><href>/dav/a</href><href>/dav/b</href><status>HTTP/1.1 200 OK</status></response></multistatus>"#,
    ] {
        assert!(parse(xml).is_err(), "{xml}");
    }
}
#[test]
fn href_confinement_rejects_origin_escape_and_encoded_separators() {
    let endpoint = Endpoint::new("https://fixture.invalid/dav/").unwrap();
    for href in [
        "https://evil.invalid/dav/a",
        "http://fixture.invalid/dav/a",
        "//evil.invalid/a",
        "/dav-other/a",
        "/secret",
        "../secret",
        "%2e%2e/secret",
        "/dav/a%2Fb",
        "/dav/a%5cb",
        "/dav/a%0db",
        "/dav/%zz",
        "/dav/a?token=secret",
        "/dav/a#fragment",
        "https://user:secret@fixture.invalid/dav/a",
    ] {
        assert!(
            endpoint.resolve_href(endpoint.as_str(), href).is_err(),
            "{href}"
        );
    }
    assert_eq!(
        endpoint.resolve_href(endpoint.as_str(), "a%20b").unwrap(),
        "https://fixture.invalid/dav/a%20b"
    );
}
#[test]
fn decoded_segments_are_encoded_once() {
    assert_eq!(
        ResourcePath::from_segments(["café + 100%", "%2F"], false)
            .unwrap()
            .as_encoded_str(),
        "caf%C3%A9%20+%20100%25/%252F"
    );
    for segment in ["..", ".", "a/b", "a\\b", "bad\n"] {
        assert!(ResourcePath::from_segments([segment], false).is_err());
    }
}
#[test]
fn endpoints_require_tls_without_secret_url_components() {
    for url in [
        "http://example.com/dav",
        "https://u:p@example.com/dav",
        "https://example.com/dav?q=x",
        "https://example.com/dav#x",
    ] {
        assert!(Endpoint::new(url).is_err());
    }
    assert!(Endpoint::loopback("http://localhost/dav").is_err());
    assert!(Endpoint::loopback("http://127.0.0.1/dav").is_ok());
}
#[test]
fn etags_are_opaque_and_weak_tags_remain_weak() {
    assert!(EntityTag::parse("W/\"abc\"").unwrap().is_weak());
    assert!(!EntityTag::parse("\"abc\"").unwrap().is_weak());
    for bad in ["abc", "w/\"a\"", "\"a\r\nb\"", "\"a b\"", "\"a\"b\""] {
        assert!(EntityTag::parse(bad).is_err());
    }
}
#[test]
fn xml_limits_do_not_silently_truncate_listings() {
    let xml = r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/dav/a</d:href><d:status>HTTP/1.1 404 Gone</d:status></d:response></d:multistatus>"#;
    let endpoint = Endpoint::new("https://fixture.invalid/dav/").unwrap();
    for limits in [
        XmlLimits {
            bytes: 10,
            ..Default::default()
        },
        XmlLimits {
            resources: 0,
            ..Default::default()
        },
        XmlLimits {
            nodes: 2,
            ..Default::default()
        },
        XmlLimits {
            depth: 1,
            ..Default::default()
        },
    ] {
        assert!(MultiStatus::parse(xml.as_bytes(), &endpoint, endpoint.as_str(), &limits).is_err());
    }
}
