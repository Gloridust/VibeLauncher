//! `vibelauncher` — the small program every shortcut and link handler runs.
//!
//! It links no GUI toolkit, so opening a profile costs a few milliseconds and a
//! few megabytes. Running it with no arguments opens the manager window
//! (`vibelauncher-gui`).

// No console window flashing up when a Windows shortcut runs us; we attach to
// the parent's console when started from a terminal.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    vibelauncher::main_with(std::env::args().skip(1).collect())
}
