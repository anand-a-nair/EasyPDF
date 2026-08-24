//! The sandboxed worker process.
//!
//! Reads length-prefixed requests on stdin, writes responses on stdout, and
//! never touches anything else. See `ideas/07-security.md` and decision D-005.
//!
//! **Startup order is a security property.** The process confines itself
//! before the first read, so a document cannot influence the confinement that
//! is meant to contain it.
//!
//! No PDF parsing happens here yet — PDFium is not vendored (TD-007). What
//! exists is the boundary itself: the process, the confinement, the protocol,
//! and honest refusals for everything not yet implemented.

// Tests legitimately assert on known-good values; the panic lints exist to
// keep unwraps out of parsing paths, not out of assertions.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod sandbox;
#[cfg(debug_assertions)]
mod selftest;

use std::io::{self, BufReader, BufWriter};

use easypdf_ffi::framing::{FrameError, read_frame, write_frame};
use easypdf_ffi::protocol::{Request, Response, WorkerError};

fn main() {
    // Before anything else. Nothing above this line may touch input.
    let sandbox_status = sandbox::apply();

    if !sandbox_status.is_enforced() {
        // Loud, because it means the process handling untrusted input has
        // ordinary user privileges. The host also surfaces this to the user.
        eprintln!("easypdf-worker: WARNING — running unconfined: {sandbox_status:?}");
    }

    // Debug builds only: probe whether confinement actually denies anything.
    // A sandbox that reports success but blocks nothing is the worst outcome,
    // because everything downstream assumes it holds.
    #[cfg(debug_assertions)]
    if std::env::var_os("EASYPDF_WORKER_SELFTEST").is_some() {
        selftest::run_and_exit();
    }

    let mut stdin = BufReader::new(io::stdin());
    let mut stdout = BufWriter::new(io::stdout());

    loop {
        let request: Request = match read_frame(&mut stdin) {
            Ok(request) => request,
            // The host closed the channel or died; exit quietly.
            Err(FrameError::Closed) => break,
            Err(error) => {
                eprintln!("easypdf-worker: unreadable request: {error}");
                break;
            }
        };

        if matches!(request, Request::Shutdown) {
            break;
        }

        let response = handle(&request, &sandbox_status);

        if let Err(error) = write_frame(&mut stdout, &response) {
            eprintln!("easypdf-worker: could not send response: {error}");
            break;
        }
    }
}

fn handle(request: &Request, sandbox_status: &easypdf_ffi::protocol::SandboxStatus) -> Response {
    match request {
        Request::Handshake => Response::Ready {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            sandbox: sandbox_status.clone(),
        },

        // Everything below needs a PDF engine, which is not vendored yet.
        // Refusing clearly beats returning plausible-looking nonsense — see
        // the correctness principle in ideas/01-vision.md.
        Request::OpenDocument { .. } => Response::Failed(WorkerError::Unsupported(
            "document parsing is not implemented yet (PDFium not vendored — TD-007)".to_owned(),
        )),

        Request::RenderPage { .. } => Response::Failed(WorkerError::Unsupported(
            "page rendering is not implemented yet (PDFium not vendored — TD-007)".to_owned(),
        )),

        Request::ExtractText { .. } => Response::Failed(WorkerError::Unsupported(
            "text extraction is not implemented yet (PDFium not vendored — TD-007)".to_owned(),
        )),

        Request::CloseDocument => Response::Ok,

        // Handled by the caller before reaching here.
        Request::Shutdown => Response::Ok,

        // `Request` is non-exhaustive, so a host newer than this worker can
        // send something unknown. Refuse explicitly: silently returning Ok
        // would make a version mismatch look like success.
        unknown => Response::Failed(WorkerError::Unsupported(format!(
            "request not understood by this worker version: {unknown:?}"
        ))),
    }
}
