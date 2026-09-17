//! One name for "a Linux desktop", so the modules do not each spell out
//! `all(unix, not(target_os = "macos"), not(target_os = "android"))` and
//! drift apart by a clause. Android is unix and is not macOS, so before this
//! existed every Linux arm -- GTK, D-Bus, the tray thread -- was compiled for
//! a phone, where none of it links.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(linux_desktop)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let unix = std::env::var_os("CARGO_CFG_UNIX").is_some();
    if unix && !matches!(os.as_str(), "macos" | "ios" | "android") {
        println!("cargo::rustc-cfg=linux_desktop");
    }
}
