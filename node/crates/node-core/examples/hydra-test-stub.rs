//! Stub for an external binary used by tests.
//!
//! Tests used to drop `#!/bin/sh` scripts here. The reason for replacing them is
//! **portability**: Windows is a supported target and a shebang does not work
//! there at all, so those fixtures would not have flaked, they would not have run.
//! A real binary works on both platforms and, as a side effect, removes the
//! interpreter from the exec path.
//!
//! Declared as an example rather than a `[[bin]]`: `cargo test` builds examples
//! automatically while `cargo build --release` does not build them at all. A
//! host-only artefact must not land beside the product binaries.
//!
//! What to emit is read from files next to the stub, not from environment
//! variables. Environment is per-process, so parallel tests expecting different
//! strings would race for it.
//!
//! Up to three sidecar files are read:
//!
//! - `<path>.out`  -> written to stdout;
//! - `<path>.err`  -> written to stderr;
//! - `<path>.code` -> exit status.
//!
//! With none of them present the stub emits nothing and exits 0, which covers the
//! fixture that only needs a successful run.

use std::io::Write;

fn main() {
    let Ok(own_path) = std::env::current_exe() else {
        return;
    };

    let sidecar = |suffix: &str| {
        let mut path = own_path.clone().into_os_string();
        path.push(suffix);
        std::fs::read(&path).ok()
    };

    if let Some(payload) = sidecar(".out") {
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(&payload);
        let _ = stdout.flush();
    }
    if let Some(payload) = sidecar(".err") {
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(&payload);
        let _ = stderr.flush();
    }

    let code = sidecar(".code")
        .and_then(|raw| String::from_utf8(raw).ok())
        .and_then(|raw| raw.trim().parse::<i32>().ok())
        .unwrap_or(0);
    std::process::exit(code);
}
