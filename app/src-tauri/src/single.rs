//! One manager window per user. A second start hands its arguments (for
//! example `--choose-route <url>`) to the window that is already open and
//! exits; the open window then comes to the front.

use std::io::{BufRead, BufReader, Write};

use interprocess::local_socket::{prelude::*, GenericNamespaced, Listener, ListenerOptions, Stream};

fn name() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    let user: String = user.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!("io.github.gloridust.vibelauncher.{user}.sock")
}

pub enum Start {
    /// We are the window; serve later starts with this (if listening worked).
    First(Option<Listener>),
    /// Another window took the arguments.
    Handed,
}

pub fn start(args: &[String]) -> Start {
    let Ok(ns) = name().to_ns_name::<GenericNamespaced>() else { return Start::First(None) };
    if let Ok(mut s) = Stream::connect(ns.clone()) {
        let line = serde_json::to_string(args).unwrap_or_else(|_| "[]".into());
        if s.write_all(format!("{line}\n").as_bytes()).is_ok() {
            // Wait for the other side to read it before going away.
            let mut ack = String::new();
            let _ = BufReader::new(&mut s).read_line(&mut ack);
            return Start::Handed;
        }
    }
    // Couldn't listen: still open a window rather than nothing.
    Start::First(ListenerOptions::new().name(ns).try_overwrite(true).create_sync().ok())
}

/// Run `on_args` (on a background thread) for every later start.
pub fn serve(listener: Listener, on_args: impl Fn(Vec<String>) + Send + 'static) {
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let mut r = BufReader::new(conn);
            let mut line = String::new();
            if r.read_line(&mut line).is_ok() {
                let _ = r.get_mut().write_all(b"ok\n");
                on_args(serde_json::from_str(&line).unwrap_or_default());
            }
        }
    });
}
