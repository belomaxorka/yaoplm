// Release builds are GUI apps on Windows: no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(err) = yaoplm_desktop::run() {
        eprintln!("yaoplm: {err}");
        std::process::exit(1);
    }
}
