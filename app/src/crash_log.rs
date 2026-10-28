//! Crash diagnostics: on Windows GUI builds there is no console, so a Rust
//! panic is a silent vanishing act. This module appends boot/lifecycle
//! lines and any panic payload to `videopsalm-debug.log` next to the
//! executable (OS temp dir as fallback when the install folder is not
//! writable). Best-effort only: a failed write just drops the line, and
//! nothing here allocates on failure paths or panics itself.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

fn log_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("videopsalm-debug.log");
            // Probe writability once: a Program Files install may be
            // read-only, in which case fall through to temp.
            if std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
                .is_ok()
            {
                return p;
            }
        }
    }
    std::env::temp_dir().join("videopsalm-debug.log")
}

fn path() -> &'static PathBuf {
    static P: OnceLock<PathBuf> = OnceLock::new();
    P.get_or_init(log_path)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Append one timestamped line. Never panics; tiny best-effort write.
pub fn log_line(msg: &str) {
    let line = format!("{} {}\n", now_secs(), msg);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path())
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Install a panic hook that keeps the default stderr report AND appends the
/// payload to the debug log, so a GUI crash leaves a trace the operator can
/// send back instead of a silent exit.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        log_line(&format!("PANIC: {info}"));
    }));
}
