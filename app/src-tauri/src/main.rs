//! `vibelauncher-gui` — VibeLauncher's manager window.
//!
//! It isn't resident: no tray, no background service. Shortcuts and link
//! handlers run the small `vibelauncher` command instead, and this window
//! exits when it is closed. When run with one of that command's arguments
//! (as the AppImage is, from its shortcuts) it behaves exactly like it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod update;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| vibelauncher::is_command(a)) {
        return vibelauncher::main_with(args);
    }
    // glibc gives every thread that allocates its own arena (up to 8 per
    // CPU); WebKit and GTK start many threads, so cap it before they exist.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
    commands::run(args);
    std::process::ExitCode::SUCCESS
}
