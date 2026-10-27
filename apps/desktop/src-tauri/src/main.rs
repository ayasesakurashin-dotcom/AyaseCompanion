#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod conversation;
mod environment;
mod host;
mod memory;
mod services;
#[cfg(any(test, feature = "smoke-test"))]
mod test_http;
fn main() {
    host::run()
}
