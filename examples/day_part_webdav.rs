// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
// Developer command-line example; diagnostics and protocol data are not application UI.
use day_part_webdav::{Authentication, Client, Depth, Endpoint, ResourcePath};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = Endpoint::new(&std::env::var("DAV_ENDPOINT")?)?;
    let client = Client::builder(endpoint)
        .authentication(Authentication::Basic {
            username: std::env::var("DAV_USERNAME")?,
            password: std::env::var("DAV_PASSWORD")?,
        })
        .build();
    let listing =
        futures_lite::future::block_on(client.propfind(&ResourcePath::root(), Depth::One))?;
    for resource in listing.resources {
        println!("{} {:?}", resource.href, resource.content_length());
    }
    Ok(())
}
