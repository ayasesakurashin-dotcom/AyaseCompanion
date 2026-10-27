#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod environment;
mod host;
fn main() {
    host::run()
}
