// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use crate::{Endpoint, Error};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct XmlLimits {
    pub bytes: usize,
    pub nodes: u32,
    pub depth: usize,
    pub resources: usize,
}
impl Default for XmlLimits {
    fn default() -> Self {
        Self {
            bytes: 8 * 1024 * 1024,
            // Apache's ordinary six-property responses use ~39 nodes per resource.
            // Keep the node bound compatible with the 10,000-resource / 8 MiB bounds.
            nodes: 400_000,
            depth: 64,
            resources: 10_000,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyName {
    pub namespace: String,
    pub local: String,
}
#[derive(Clone, Debug)]
pub struct Property {
    pub name: PropertyName,
    /// Concatenated text; untrusted data, never HTML.
    pub text: String,
    /// Direct child expanded names, e.g. DAV:collection in DAV:resourcetype.
    pub children: Vec<PropertyName>,
    /// Original fragment for extensions. Namespace declarations may be on ancestors.
    pub xml: String,
}
#[derive(Clone, Debug)]
pub struct PropertyStatus {
    pub status: u16,
    pub properties: Vec<Property>,
}
#[derive(Clone, Debug)]
pub struct Resource {
    pub href: String,
    pub status: Option<u16>,
    pub properties: Vec<PropertyStatus>,
}
impl Resource {
    /// Only a successful property block may supply a value.
    pub fn property(&self, namespace: &str, local: &str) -> Option<&Property> {
        self.properties
            .iter()
            .filter(|p| (200..300).contains(&p.status))
            .flat_map(|p| &p.properties)
            .find(|p| p.name.namespace == namespace && p.name.local == local)
    }
    pub fn is_collection(&self) -> bool {
        self.property("DAV:", "resourcetype").is_some_and(|p| {
            p.children
                .iter()
                .any(|n| n.namespace == "DAV:" && n.local == "collection")
        })
    }
    pub fn content_length(&self) -> Option<u64> {
        self.property("DAV:", "getcontentlength")?
            .text
            .trim()
            .parse()
            .ok()
    }
    pub fn etag(&self) -> Option<&str> {
        self.property("DAV:", "getetag").map(|p| p.text.trim())
    }
}
#[derive(Clone, Debug)]
pub struct MultiStatus {
    pub resources: Vec<Resource>,
    pub sync_token: Option<String>,
}
impl MultiStatus {
    pub fn has_failures(&self) -> bool {
        self.resources.iter().any(|r| {
            r.status.is_some_and(|s| !(200..300).contains(&s))
                || r.properties.iter().any(|p| !(200..300).contains(&p.status))
        })
    }
    pub fn parse(
        bytes: &[u8],
        endpoint: &Endpoint,
        requested: &str,
        limits: &XmlLimits,
    ) -> Result<Self, Error> {
        if bytes.len() > limits.bytes {
            return Err(Error::Limit);
        }
        let xml = std::str::from_utf8(bytes).map_err(|_| Error::Xml)?;
        let doc = Document::parse_with_options(
            xml,
            ParsingOptions {
                allow_dtd: false,
                nodes_limit: limits.nodes,
                ..Default::default()
            },
        )
        .map_err(|e| {
            if matches!(e, roxmltree::Error::NodesLimitReached) {
                Error::Limit
            } else {
                Error::Xml
            }
        })?;
        let root = doc.root_element();
        if !root.has_tag_name(("DAV:", "multistatus")) {
            return Err(Error::Xml);
        }
        for node in root.descendants() {
            if node
                .ancestors()
                .take(limits.depth.saturating_add(2))
                .count()
                > limits.depth.saturating_add(1)
            {
                return Err(Error::Limit);
            }
        }
        let mut resources = Vec::new();
        let mut seen = HashSet::new();
        for r in root
            .children()
            .filter(|n| n.has_tag_name(("DAV:", "response")))
        {
            if resources.len() >= limits.resources {
                return Err(Error::Limit);
            }
            let href = one(r, "href")?.text().ok_or(Error::Xml)?.trim();
            let href = endpoint.resolve_href(requested, href)?;
            if !seen.insert(href.clone()) {
                return Err(Error::Xml);
            }
            let statuses: Vec<_> = r
                .children()
                .filter(|n| n.has_tag_name(("DAV:", "status")))
                .collect();
            if statuses.len() > 1 {
                return Err(Error::Xml);
            }
            let status = statuses
                .first()
                .map(|n| parse_status(n.text().unwrap_or("")))
                .transpose()?;
            let mut properties = Vec::new();
            for ps in r
                .children()
                .filter(|n| n.has_tag_name(("DAV:", "propstat")))
            {
                let status = parse_status(one(ps, "status")?.text().ok_or(Error::Xml)?)?;
                let prop = one(ps, "prop")?;
                let values = prop
                    .children()
                    .filter(Node::is_element)
                    .map(|p| Property {
                        name: name(p),
                        text: p
                            .descendants()
                            .filter(Node::is_text)
                            .filter_map(|n| n.text())
                            .collect(),
                        children: p.children().filter(Node::is_element).map(name).collect(),
                        xml: xml[p.range()].into(),
                    })
                    .collect();
                properties.push(PropertyStatus {
                    status,
                    properties: values,
                });
            }
            if (status.is_some() && !properties.is_empty())
                || (status.is_none() && properties.is_empty())
            {
                return Err(Error::Xml);
            }
            resources.push(Resource {
                href,
                status,
                properties,
            });
        }
        let sync_token = root
            .children()
            .find(|n| n.has_tag_name(("DAV:", "sync-token")))
            .and_then(|n| n.text())
            .map(str::to_owned);
        Ok(Self {
            resources,
            sync_token,
        })
    }
}
fn one<'a, 'i>(node: Node<'a, 'i>, local: &str) -> Result<Node<'a, 'i>, Error> {
    let mut found = node.children().filter(|n| n.has_tag_name(("DAV:", local)));
    let result = found.next().ok_or(Error::Xml)?;
    if found.next().is_some() {
        return Err(Error::Xml);
    }
    Ok(result)
}
fn name(node: Node<'_, '_>) -> PropertyName {
    PropertyName {
        namespace: node.tag_name().namespace().unwrap_or("").into(),
        local: node.tag_name().name().into(),
    }
}
fn parse_status(raw: &str) -> Result<u16, Error> {
    let mut parts = raw.split_ascii_whitespace();
    if !matches!(
        parts.next(),
        Some("HTTP/1.0" | "HTTP/1.1" | "HTTP/2" | "HTTP/2.0")
    ) {
        return Err(Error::Xml);
    }
    let code = parts.next().ok_or(Error::Xml)?;
    if code.len() != 3 {
        return Err(Error::Xml);
    }
    let code = code.parse().map_err(|_| Error::Xml)?;
    if !(100..600).contains(&code) {
        return Err(Error::Xml);
    }
    Ok(code)
}
