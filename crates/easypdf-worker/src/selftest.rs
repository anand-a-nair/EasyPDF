//! Confinement self-check. Debug builds only.
//!
//! Verifies that the sandbox **denies** operations, rather than trusting
//! `sandbox_init`'s return code. A sandbox that reports success while blocking
//! nothing is worse than none, because the architecture assumes it holds.
//!
//! Runs after confinement is applied and exits without entering the request
//! loop, so it never coexists with real document handling.

use std::io::Write;
use std::net::TcpListener;

/// Runs each probe, prints one `name=allowed|denied` line per result, exits.
pub(crate) fn run_and_exit() -> ! {
    let results = [
        ("read_etc_passwd", probe_file_read()),
        ("write_temp_file", probe_file_write()),
        ("network_connect", probe_network()),
    ];

    let mut stdout = std::io::stdout();
    for (name, allowed) in results {
        let verdict = if allowed { "allowed" } else { "denied" };
        let _ = writeln!(stdout, "{name}={verdict}");
    }
    let _ = stdout.flush();

    std::process::exit(0);
}

/// Returns true if the operation succeeded, i.e. was *not* blocked.
fn probe_file_read() -> bool {
    // Reading an arbitrary file requires file-read-data, which the profile
    // does not grant.
    std::fs::read_to_string("/etc/passwd").is_ok()
}

fn probe_file_write() -> bool {
    let path = std::env::temp_dir().join("easypdf-worker-selftest.tmp");
    let wrote = std::fs::write(&path, b"should not be possible").is_ok();
    let _ = std::fs::remove_file(&path);
    wrote
}

fn probe_network() -> bool {
    // Binding a listener rather than connecting out: a connect attempt fails
    // when nothing is listening, so it cannot distinguish "sandbox blocked it"
    // from "nobody answered". Binding an ephemeral port succeeds
    // unconditionally when unconfined, so a failure here is attributable to
    // the sandbox.
    TcpListener::bind("127.0.0.1:0").is_ok()
}
