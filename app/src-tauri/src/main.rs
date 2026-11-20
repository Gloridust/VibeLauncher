//! `vibelauncher-gui` — VibeLauncher's manager window.
//!
//! It isn't resident: no tray, no background service. Shortcuts and link
//! handlers run the small `vibelauncher` command instead, and this window
//! exits when it is closed. When run with one of that command's arguments
//! (as the AppImage is, from its shortcuts) it behaves exactly like it.
//!
//! The window is drawn with Slint's software renderer: no GPU, no WebKit, so
//! graphics drivers can't leave it garbled or see-through.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod app;
mod i18n;
#[cfg(feature = "selftest")]
mod selftest;
mod single;
mod update;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| vibelauncher::is_command(a)) {
        return vibelauncher::main_with(args);
    }
    #[cfg(feature = "selftest")]
    if let Some(out) = std::env::var_os("VL_SELFTEST_OUT") {
        return selftest::click_through(out.into());
    }
    #[cfg(feature = "selftest")]
    if let Some(dir) = std::env::var_os("VL_SHOT") {
        return selftest::run(dir.into());
    }

    let listener = match single::start(&args) {
        single::Start::Handed => return std::process::ExitCode::SUCCESS,
        single::Start::First(l) => l,
    };

    #[allow(unused_mut)]
    let mut backend = slint::BackendSelector::new().backend_name("winit".into()).renderer_name("software".into());
    #[cfg(target_os = "macos")]
    {
        // Keep the traffic lights, but over the window's own colours.
        backend = backend.with_winit_window_attributes_hook(|a| {
            use slint::winit_030::winit::platform::macos::WindowAttributesExtMacOS;
            a.with_titlebar_transparent(true).with_fullsize_content_view(true).with_title_hidden(true)
        });
    }
    if let Err(e) = backend.select() {
        eprintln!("VibeLauncher: {e}");
        return std::process::ExitCode::FAILURE;
    }

    let win = match AppWindow::new() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("VibeLauncher: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    app::install(&win, &args);
    if let Some(l) = listener {
        single::serve(l, app::on_second_start);
    }
    if let Err(e) = win.run() {
        eprintln!("VibeLauncher: {e}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
