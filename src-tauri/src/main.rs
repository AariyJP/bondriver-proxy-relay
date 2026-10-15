// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use epgsync_lib::{bondriver, sync};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--config") && args.len() >= 2 {
        unsafe { std::env::set_var("EPGSYNC_CONFIG", &args[1]) };
        args.drain(..2);
    }
    let result = match args.first().map(String::as_str) {
        Some("serve") => sync::serve(),
        Some("pull") => sync::load().map(|cfg| sync::pull(&cfg).iter().for_each(|l| println!("{l}"))),
        Some("apply") => bondriver::apply_cli(),
        Some("relay") => bondriver::relay(args.get(1).map(String::as_str).unwrap_or_default()),
        _ => return epgsync_lib::run(),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
