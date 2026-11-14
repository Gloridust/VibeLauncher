//! A stand-in for the Claude desktop app, used only by the smoke test
//! (`cargo test -p vibelauncher --features smoke-test`). It records how it was
//! started — arguments and the isolation variables — in `fake-app-log.jsonl`
//! next to its executable, then stays running like a real app until it is
//! asked to quit or `fake-app-stop` appears next to it.

use std::io::Write;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = std::env::current_exe().expect("own path");
    let dir = exe.parent().expect("own folder").to_path_buf();
    let mut env = serde_json::Map::new();
    for k in ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "CODEX_ELECTRON_USER_DATA_PATH"] {
        if let Ok(v) = std::env::var(k) {
            env.insert(k.into(), v.into());
        }
    }
    let line = serde_json::json!({ "pid": std::process::id(), "args": args, "env": env }).to_string() + "\n";
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("fake-app-log.jsonl"))
        .expect("log file");
    f.write_all(line.as_bytes()).expect("write log");
    drop(f);

    let stop = dir.join("fake-app-stop");
    let deadline = Instant::now() + Duration::from_secs(600);
    while Instant::now() < deadline && !stop.exists() {
        std::thread::sleep(Duration::from_millis(200));
    }
}
