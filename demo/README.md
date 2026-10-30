# WebDAV Explorer

A conventional Day demo for `day-part-webdav`, scaffolded with `day new app`. It is a practical
protocol workbench: inspect collections and headers, test conditional uploads, copy/move/delete,
compare HTTP providers, cancel slow requests, and inspect per-property DAV failures.

## Run

```sh
day patch --local ../../day
# Uses native URLSession, then reqwest, against the app's own loopback fixture:
day launch -t macos-appkit --script dayscript/demo.yaml
```

Use any target declared in `Day.toml`. Native apps host a synthetic in-memory DAV fixture within
their own process, so Android and iOS need no host-port forwarding. The browser cannot host a TCP
server; start the host fixture first:

```sh
# In the parent directory, keep this process running:
cargo run --features demo-server --example fixture_server
# In demo/:
day launch -t web-dom --script dayscript/demo.yaml
```

CI starts and stops that browser fixture automatically. The app's loopback HTTP exception is for
local testing; remote endpoints require HTTPS and ordinary platform certificate validation.

## Use

Connection selects Native or Rust reqwest, authentication, and the collection URL. For Nextcloud,
use a dedicated collection under `https://host/remote.php/dav/files/USERNAME/` with an app password.
The provider switch affects subsequent operations; cancellation stops the current one first.
Web and HarmonyOS display their native-only limitation.

Resources accepts decoded paths relative to the connection root. A literal `+` or `%` is a filename
character, not an escape instruction. Select a listed row to populate its path and ETag. Upload is
create-only; Replace requires a strong ETag. Copy and Move refuse overwrite. Delete is explicit and
refuses the connection root, but deleting a child collection may delete its entire subtree.

Response shows the last protocol result, headers/body preview (up to 16 KiB) or parsed DAV property
blocks, plus cumulative session transfer statistics. It is a bounded inspector, not a raw packet
capture: authentication headers are not displayed, and error bodies are not retained by the client.
Server content can still be private; nothing is saved automatically. Screenshots explicitly requested
by dayscript do save the visible diagnostic data, so use synthetic fixtures for CI.

Run protocol checks creates a unique disposable child collection, exercises file operations and
conditional/range semantics, and attempts cleanup even after a failure. Use local fixture resets the
connection to the in-memory server. Network-policy switches and slow/interrupted fixture reads are
available for experiments. Fixture endpoints `__slow__`, `__drop__` and `__busy__` simulate latency,
truncation and temporary overload; they have no special meaning on an external server.

The fixture is intentionally small and is not an authenticated production DAV server. Apache and
Nextcloud compatibility evidence is captured separately by `../scripts/capture.py --native`.
