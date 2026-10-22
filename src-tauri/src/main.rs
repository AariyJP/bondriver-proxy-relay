// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bondriverproxyrelay_lib::bondriver;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = args.get(1).map(String::as_str).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("apply") => bondriver::apply_cli(arg),
        Some("relay") => bondriver::relay(arg),
        Some(other) => Err(format!("unknown command: {other}")),
        None => return bondriverproxyrelay_lib::run(),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
