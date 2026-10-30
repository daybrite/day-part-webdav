#!/usr/bin/env bash
# Sourced from demo/ by the standard Day workflow; native apps host their own fixture.
if [[ "${DAY_SCRIPT_TARGET:-}" == "web-dom" ]]; then
  (cd .. && cargo build --target-dir target --features demo-server --example fixture_server)
  ../target/debug/examples/fixture_server > "${RUNNER_TEMP:-/tmp}/day-webdav-fixture.log" 2>&1 &
  day_webdav_fixture_pid=$!
  trap 'kill "$day_webdav_fixture_pid" 2>/dev/null || true; wait "$day_webdav_fixture_pid" 2>/dev/null || true' EXIT
  day_webdav_fixture_ready=false
  for day_webdav_attempt in {1..100}; do
    kill -0 "$day_webdav_fixture_pid" || { cat "${RUNNER_TEMP:-/tmp}/day-webdav-fixture.log"; exit 1; }
    if curl --silent --fail http://127.0.0.1:18444/dav/fixture.txt >/dev/null; then
      day_webdav_fixture_ready=true
      break
    fi
    sleep 0.1
  done
  "$day_webdav_fixture_ready" || { echo 'WebDAV fixture failed to become ready' >&2; exit 1; }
fi
