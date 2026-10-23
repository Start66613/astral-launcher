#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|a| a == "--selftest") {
        let fails = astral_lib::selftest();
        std::process::exit(fails);
    }
    astral_lib::run()
}
