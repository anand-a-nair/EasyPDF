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
use std::path::PathBuf;

use easypdf_ffi::framing::{FrameError, read_frame, write_blob, write_frame};
use easypdf_ffi::protocol::{Request, Response, SandboxStatus, WorkerError};
use easypdf_render::cache::{TileKey, ZoomBucket};
use easypdf_render::pdfium::{OpenDocument, PdfiumRasterizer};

/// Worker state: the loaded engine and the document currently open.
///
/// The document is held **parsed**, not as bytes. Re-parsing per request was
/// costing a full document parse for every page render, size query and search.
struct Session {
    rasterizer: Option<PdfiumRasterizer>,
    document: Option<OpenDocument>,
}

/// Where the PDFium library lives, relative to this executable.
///
/// Loading it needs filesystem access, so it happens **before** the sandbox is
/// applied. That is not a hole in the model: a hash-pinned library loaded at
/// startup is not untrusted input, and confinement still lands before the first
/// document byte is read.
///
/// Two layouts have to work, and they are checked in a fixed order so a
/// development build never silently picks up a stale bundled copy:
///
///   development  target/<profile>/  ->  repo root  ->  vendor/pdfium/<target>/lib
///   bundled      the library sits beside the executable, or one level up in a
///                platform resource directory (macOS: Contents/Resources)
fn pdfium_directory() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let executable_dir = executable.parent()?;

    let target = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "mac-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "mac-x64"
    } else if cfg!(target_os = "windows") {
        "win-x64"
    } else {
        "linux-x64"
    };

    let library = if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else if cfg!(target_os = "windows") {
        "pdfium.dll"
    } else {
        "libpdfium.so"
    };

    let candidates = [
        // Development: repo root is two levels above target/<profile>/.
        executable_dir
            .parent()
            .and_then(std::path::Path::parent)
            .map(|root| root.join("vendor/pdfium").join(target).join("lib")),
        // Bundled: beside the executable.
        Some(executable_dir.to_path_buf()),
        // Bundled on macOS: Contents/MacOS/../Resources. Tauri preserves the
        // staging directory name, so the library sits in a `resources`
        // subdirectory rather than directly in Resources.
        executable_dir.parent().map(|parent| parent.join("Resources/resources")),
        executable_dir.parent().map(|parent| parent.join("Resources")),
        // Bundled on Linux/Windows: a resources directory beside the binary.
        Some(executable_dir.join("resources")),
        // Bundled on Linux: alongside a lib/ sibling.
        executable_dir.parent().map(|parent| parent.join("lib")),
    ];

    candidates.into_iter().flatten().find(|directory| directory.join(library).is_file())
}

fn main() {
    // Load the engine first: binding the library needs file access, which the
    // sandbox is about to remove.
    let rasterizer =
        pdfium_directory().and_then(|directory| match PdfiumRasterizer::load_from(&directory) {
            Ok(rasterizer) => Some(rasterizer),
            Err(error) => {
                eprintln!("easypdf-worker: PDFium unavailable: {error}");
                None
            }
        });

    // Now confine. Nothing below this line has touched input yet.
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

    let mut session = Session { rasterizer, document: None };

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

        let response = handle(&request, &sandbox_status, &mut session);

        if let Err(error) = send(&mut stdout, response) {
            // Previously this exited the loop, which killed the worker for the
            // rest of the session: the host's next write then failed with
            // "broken pipe" and the user saw that instead of the real problem.
            // One unsendable response must not end the process — report it and
            // carry on, so the next request still works.
            eprintln!("easypdf-worker: could not send response: {error}");

            let refusal = Response::Failed(WorkerError::Malformed(format!(
                "the result could not be sent: {error}"
            )));
            if write_frame(&mut stdout, &refusal).is_err() {
                // The channel itself is gone; there is nothing left to say.
                break;
            }
        }
    }
}

/// Sends a response, with pixels as a separate binary frame.
fn send<W: std::io::Write>(writer: &mut W, response: Response) -> Result<(), FrameError> {
    match response {
        Response::PageRendered { width, height, byte_len, pixels } => {
            write_frame(
                writer,
                &Response::PageRendered { width, height, byte_len, pixels: Vec::new() },
            )?;
            write_blob(writer, &pixels)
        }
        other => write_frame(writer, &other),
    }
}

fn handle(request: &Request, sandbox_status: &SandboxStatus, session: &mut Session) -> Response {
    match request {
        Request::Handshake => Response::Ready {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            sandbox: sandbox_status.clone(),
            // Reported at handshake so the host knows before a user opens
            // anything. A worker with no engine can only refuse, and finding
            // that out at open time — after the file dialog — is worse.
            engine_available: session.rasterizer.is_some(),
        },

        Request::OpenDocument { data, password } => {
            let Some(rasterizer) = session.rasterizer.as_ref() else {
                return Response::Failed(engine_missing());
            };

            // Drop any previous document first so its memory is released
            // before the next one is parsed, rather than holding both.
            session.document = None;

            match rasterizer.open(data.clone(), password.as_deref()) {
                Ok(document) => {
                    let page_count = document.page_count();
                    session.document = Some(document);
                    Response::DocumentOpened {
                        page_count,
                        // Reported honestly as approximate for now: telling
                        // these apart properly means reading the encryption
                        // dictionary and signature fields, which arrives with
                        // easypdf-crypto in Phase 3.
                        encrypted: password.is_some(),
                        signed: false,
                    }
                }
                // A password problem is reported as itself, not as a malformed
                // document: the two call for completely different responses
                // from the user.
                Err(easypdf_render::RenderError::PasswordRequired) => {
                    Response::Failed(WorkerError::BadPassword)
                }
                Err(error) => Response::Failed(WorkerError::Malformed(error.to_string())),
            }
        }

        Request::RenderPage { page, zoom, rotation } => {
            let Some(document) = session.document.as_ref() else {
                return Response::Failed(no_document());
            };

            let key =
                TileKey { page: *page, zoom: ZoomBucket::from_zoom(*zoom), rotation: *rotation };

            match document.render(key) {
                Ok(tile) => Response::PageRendered {
                    width: tile.width,
                    height: tile.height,
                    byte_len: tile.pixels.len(),
                    pixels: tile.pixels,
                },
                Err(error) => Response::Failed(WorkerError::Malformed(error.to_string())),
            }
        }

        Request::PageSize { page } => {
            let Some(document) = session.document.as_ref() else {
                return Response::Failed(no_document());
            };

            match document.page_size(*page) {
                Ok((width, height)) => Response::PageSize { width, height },
                Err(error) => Response::Failed(WorkerError::Malformed(error.to_string())),
            }
        }

        Request::ExtractText { page } => {
            let Some(document) = session.document.as_ref() else {
                return Response::Failed(no_document());
            };

            match document.extract_text(*page) {
                Ok(text) => Response::TextExtracted { text },
                Err(error) => Response::Failed(WorkerError::Malformed(error.to_string())),
            }
        }

        Request::TextLayout { page } => {
            let Some(document) = session.document.as_ref() else {
                return Response::Failed(no_document());
            };

            match document.text_layout(*page) {
                Ok(layout) => Response::TextLayout { layout },
                Err(error) => Response::Failed(WorkerError::Malformed(error.to_string())),
            }
        }

        Request::Outline => {
            let Some(document) = session.document.as_ref() else {
                return Response::Failed(no_document());
            };
            Response::Outline { entries: document.outline() }
        }

        Request::Search { query, match_case } => {
            let Some(document) = session.document.as_ref() else {
                return Response::Failed(no_document());
            };

            match document.search(query, *match_case) {
                Ok((hits, truncated)) => Response::SearchResults { hits, truncated },
                Err(error) => Response::Failed(WorkerError::Malformed(error.to_string())),
            }
        }

        Request::CloseDocument => {
            session.document = None;
            Response::Ok
        }

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

/// No document is open. Distinct from a malformed one: this is a caller bug,
/// not a bad file.
fn no_document() -> WorkerError {
    WorkerError::Malformed("no document is open".to_owned())
}

/// The engine failed to load. Never falls back to in-process parsing (D-017).
fn engine_missing() -> WorkerError {
    WorkerError::Unsupported(
        "the PDF engine is not available to this worker (missing from the \
         installation, or scripts/fetch-pdfium.sh has not been run)"
            .to_owned(),
    )
}
