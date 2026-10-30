// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use crate::Error;
use percent_encoding::percent_decode_str;
use url::Url;
/// An encoded path relative to an endpoint. Construct from decoded segments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourcePath(pub(crate) String);
impl ResourcePath {
    pub fn root() -> Self {
        Self(String::new())
    }
    pub fn from_segments<I, S>(segments: I, collection: bool) -> Result<Self, Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut url = Url::parse("https://fixture.invalid/").unwrap();
        {
            let mut path = url.path_segments_mut().unwrap();
            path.clear();
            for segment in segments {
                let s = segment.as_ref();
                if s.is_empty()
                    || matches!(s, "." | "..")
                    || s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
                {
                    return Err(Error::InvalidPath);
                }
                path.push(s);
            }
            if collection {
                path.push("");
            }
        }
        Ok(Self(url.path().trim_start_matches('/').into()))
    }
    pub fn as_encoded_str(&self) -> &str {
        &self.0
    }
}
/// A DAV collection boundary. Credentials, queries and fragments are excluded.
#[derive(Clone, Debug)]
pub struct Endpoint(pub(crate) Url);
impl Endpoint {
    pub fn new(url: &str) -> Result<Self, Error> {
        Self::parse(url, false)
    }
    /// Cleartext is allowed only for numeric loopback addresses in isolated tests.
    pub fn loopback(url: &str) -> Result<Self, Error> {
        Self::parse(url, true)
    }
    fn parse(raw: &str, loopback: bool) -> Result<Self, Error> {
        reject_ambiguous(raw)?;
        let mut url = Url::parse(raw).map_err(|_| Error::InvalidEndpoint)?;
        let local = url
            .host_str()
            .is_some_and(|s| matches!(s, "127.0.0.1" | "[::1]"));
        if (url.scheme() != "https" && !(loopback && local && url.scheme() == "http"))
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::InvalidEndpoint);
        }
        canonicalize_path(&mut url)?;
        if !url.path().ends_with('/') {
            let p = format!("{}/", url.path());
            url.set_path(&p);
        }
        Ok(Self(url))
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    pub fn url(&self, path: &ResourcePath) -> String {
        format!("{}{}", self.0, path.0)
    }
    pub fn resolve_href(&self, requested: &str, href: &str) -> Result<String, Error> {
        reject_ambiguous(href)?;
        let base = Url::parse(requested).map_err(|_| Error::UnsafeHref)?;
        let mut url = base.join(href).map_err(|_| Error::UnsafeHref)?;
        canonicalize_path(&mut url)?;
        if url.origin() != self.0.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !(url.path().starts_with(self.0.path())
                || url.path() == self.0.path().trim_end_matches('/'))
        {
            return Err(Error::UnsafeHref);
        }
        Ok(url.into())
    }
}
fn reject_ambiguous(raw: &str) -> Result<(), Error> {
    if raw.chars().any(|c| c.is_control() || c == '\\') {
        return Err(Error::UnsafeHref);
    }
    let path = raw.split(['?', '#']).next().unwrap_or(raw);
    for s in path.split('/') {
        let s = percent_decode_str(s)
            .decode_utf8()
            .map_err(|_| Error::UnsafeHref)?;
        if matches!(s.as_ref(), "." | "..") {
            return Err(Error::UnsafeHref);
        }
    }
    Ok(())
}
fn validate_encoded_path(path: &str) -> Result<(), Error> {
    for segment in path.split('/') {
        let b = segment.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' {
                if i + 2 >= b.len()
                    || !b[i + 1].is_ascii_hexdigit()
                    || !b[i + 2].is_ascii_hexdigit()
                {
                    return Err(Error::UnsafeHref);
                }
                i += 2;
            }
            i += 1;
        }
        let decoded = percent_decode_str(segment)
            .decode_utf8()
            .map_err(|_| Error::UnsafeHref)?;
        if matches!(decoded.as_ref(), "." | "..")
            || decoded
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\'))
        {
            return Err(Error::UnsafeHref);
        }
    }
    Ok(())
}

fn canonicalize_path(url: &mut Url) -> Result<(), Error> {
    validate_encoded_path(url.path())?;
    let segments: Vec<String> = url
        .path()
        .split('/')
        .skip(1)
        .map(|s| {
            percent_decode_str(s)
                .decode_utf8()
                .map(|s| s.into_owned())
                .map_err(|_| Error::UnsafeHref)
        })
        .collect::<Result<_, _>>()?;
    let mut path = url.path_segments_mut().map_err(|_| Error::UnsafeHref)?;
    path.clear();
    for segment in segments {
        path.push(&segment);
    }
    Ok(())
}
