// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
#[cfg(all(feature = "demo-server", not(target_arch = "wasm32")))]
fn main() {
    let _server = day_part_webdav::testing::Server::bind(18444).expect("loopback fixture port");
    println!("http://127.0.0.1:18444/dav/");
    loop {
        std::thread::park();
    }
}
#[cfg(any(not(feature = "demo-server"), target_arch = "wasm32"))]
fn main() {
    panic!("requires native demo-server feature");
}
