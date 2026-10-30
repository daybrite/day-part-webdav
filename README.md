# day-part-webdav

MPL-2.0 WebDAV foundation over `day-part-http`, created with `day new part`.
The library is headless; **[WebDAV Explorer](demo/README.md)** is its conventional Day demo.

This is an initial implementation, not a released synchronization engine. It requires the accompanying
DAV method additions in Day's `day-part-http`. Before publishing, release that dependency and raise
its minimum version here. CI currently follows Day main; publication is disabled.

## Scope

- OPTIONS, Depth 0/1 PROPFIND, GET/HEAD, MKCOL, PUT, DELETE, COPY and MOVE.
- Namespace-aware Multi-Status parsing preserving individual property failures and extensions.
- HTTPS endpoints, confined and encoded paths, opaque ETags, explicit conditional writes.
- Bounded responses/uploads/XML; cancellable async requests; bounded read retries with jitter,
  Retry-After, idle and overall deadlines; explicit constrained/expensive/offline admission.
- Validated range downloads: a 200 response means restart, never append.
- Native HTTP by default; optional `reqwest` uses the same Day HTTP session/provider abstraction.

Mutations are **never automatically replayed by this crate**. A lost response means the write may
have applied. The future persistence layer must durably record intent, read back and verify immutable
objects, resolve conflicts, and advance cursors transactionally. HTTP conditional headers do not
make arbitrary DAV servers transactional. See [research and limitations](docs/interop.html).

## Example

```rust,no_run
use day_part_webdav::{Client, Depth, Endpoint, ResourcePath};

async fn list() -> Result<(), day_part_webdav::Error> {
    let client = Client::builder(Endpoint::new("https://cloud.example/dav/private/")?).build();
    let listing = client.propfind(&ResourcePath::root(), Depth::One).await?;
    for resource in listing.resources {
        println!("{} {:?}", resource.href, resource.etag());
    }
    Ok(())
}
```

Supply `Authentication::Basic` with an app password, or `Authentication::Bearer`. Secrets remain in
memory; credential acquisition/storage belongs to the application. Use a dedicated `http::Session`
for provider switching or request interception. Enable `reqwest`, then call
`session.set_provider(http::Provider::Reqwest)` before building a client. Web and HarmonyOS support
only the native provider in the current HTTP layer.

## Develop and validate

With this crate and the modified Day checkout as siblings:

```sh
day patch --local ../day --project .
day patch --local ../day --project demo
cargo fmt --all -- --check
cargo test
cargo test --features reqwest,demo-server
cargo clippy --all-features --all-targets -- -D warnings
cd demo
day launch -t macos-appkit --script dayscript/demo.yaml
```

The root has no toolkit features. The demo declares all eight standard toolkit targets. Its
walkthrough sends real loopback HTTP through native and reqwest providers, where supported. CI
uses the shared `daylib.yml` workflow for host tests, cross checks and every declared target's demo.
No platform is silently counted as having a reqwest test when that provider is unavailable.

`tests/fixtures/*.json` are sanitized actual Apache and Nextcloud exchanges. They replay entirely
offline through `day-part-http` interception. Fault tests inject truncation, stalls, TLS errors,
rate limits and policy changes using the same HTTP simulation and a deterministic clock.
`tests/loopback.rs` additionally tests both providers against the demo fixture with real sockets.

For capture reproduction and evidence, see [docs/interop.html](docs/interop.html).
The live `tests/native.rs` tests are explicitly ignored without the capture harness, which supplies
fresh isolated server roots. No production server credentials belong in these tests or fixtures.

## Deliberate boundaries

No LOCK/UNLOCK, PROPPATCH or RFC 6578 REPORT client API yet (the HTTP layer recognizes these verbs).
No automatic redirects, infinite-depth traversal, cookie login, Digest/NTLM integration, Nextcloud
chunked upload, streaming-to-file, durable retry queue, background OS scheduling or conflict resolver.
Large files need a later streaming/chunked design: the current API bounds each buffered transfer.
Browser use additionally requires server CORS permission and exposed protocol headers.
