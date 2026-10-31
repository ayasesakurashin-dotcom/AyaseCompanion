#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod conversation;
mod environment;
mod host;
mod memory;
mod p3;
mod readable;
mod services;
mod speech_text;
#[cfg(any(test, feature = "smoke-test"))]
mod test_http;
mod voice_api;
fn main() {
    host::run()
}
