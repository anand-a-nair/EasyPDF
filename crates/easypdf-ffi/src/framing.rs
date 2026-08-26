//! Framing for the worker channel.
//!
//! Each frame is a 1-byte kind, a 4-byte big-endian length, then the payload.
//! Control messages are JSON; **pixel data is raw bytes**.
//!
//! The split is not an optimisation, it is a correctness fix. Pixels were
//! serialised inside the JSON message, where a `Vec<u8>` becomes an array of
//! decimal numbers — `255,0,17,` and so on — which is a **3.6x expansion**. A
//! single A4 page at 294% zoom on a retina display is 66 MB of pixels and was
//! becoming 236 MB of JSON. A slightly larger page crossed the frame limit,
//! the worker failed to send, and it exited; the next request then died with
//! "broken pipe". That is exactly what a real scanned document did.
//!
//! **The frame limit is a security control, not a tuning parameter.** The host
//! reads frames from a process that may be compromised; without a ceiling, a
//! hostile worker announces a 4 GB frame and the host allocates it.

use std::io::{self, Read, Write};

/// Largest accepted frame, in bytes.
///
/// Sized to admit a full-page BGRA bitmap at high zoom with headroom. A sender
/// asking for more than this is malfunctioning or hostile; either way the
/// receiver must not honor it.
pub const MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;

/// A JSON control message.
const KIND_MESSAGE: u8 = 0;

/// A raw byte payload, currently pixels.
const KIND_BLOB: u8 = 1;

/// Why a frame could not be read or written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FrameError {
    /// The declared frame length exceeds [`MAX_FRAME_BYTES`].
    #[error("frame of {declared} bytes exceeds the {MAX_FRAME_BYTES} byte limit")]
    TooLarge {
        /// The length the sender declared.
        declared: usize,
    },

    /// The channel closed, usually because the worker died.
    #[error("channel closed")]
    Closed,

    /// The payload was not valid JSON for the expected type.
    #[error("malformed frame payload: {0}")]
    Malformed(String),

    /// The frame kind byte was not recognized.
    #[error("unknown frame kind {kind}")]
    UnknownKind {
        /// The byte that was read.
        kind: u8,
    },

    /// A blob was expected but a message arrived, or the reverse.
    #[error("expected a {expected} frame, got a {actual} frame")]
    WrongKind {
        /// What the reader wanted.
        expected: &'static str,
        /// What it got.
        actual: &'static str,
    },

    /// Underlying I/O failure.
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}

/// Writes a frame header: kind byte then big-endian length.
fn write_header<W: Write>(writer: &mut W, kind: u8, length: usize) -> Result<(), FrameError> {
    if length > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge { declared: length });
    }
    let length = u32::try_from(length).map_err(|_| FrameError::TooLarge { declared: length })?;

    writer.write_all(&[kind])?;
    writer.write_all(&length.to_be_bytes())?;
    Ok(())
}

/// Reads a frame header, returning its kind and length.
fn read_header<R: Read>(reader: &mut R) -> Result<(u8, usize), FrameError> {
    let mut header = [0_u8; 5];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(FrameError::Closed);
        }
        Err(error) => return Err(FrameError::Io(error)),
    }

    let kind = header[0];
    let declared = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;

    // Validated before any allocation, so a hostile declaration costs nothing.
    if declared > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge { declared });
    }
    Ok((kind, declared))
}

fn read_exact_payload<R: Read>(reader: &mut R, length: usize) -> Result<Vec<u8>, FrameError> {
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            FrameError::Closed
        } else {
            FrameError::Io(error)
        }
    })?;
    Ok(payload)
}

/// Writes one JSON control message.
pub fn write_frame<W: Write, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), FrameError> {
    let payload =
        serde_json::to_vec(value).map_err(|error| FrameError::Malformed(error.to_string()))?;

    write_header(writer, KIND_MESSAGE, payload.len())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Writes one raw byte payload.
///
/// Used for pixels, which must never go through JSON: the encoding inflates
/// them by 3.6x and used to push a single page past the frame limit.
pub fn write_blob<W: Write>(writer: &mut W, bytes: &[u8]) -> Result<(), FrameError> {
    write_header(writer, KIND_BLOB, bytes.len())?;
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}

/// Reads one JSON control message.
pub fn read_frame<R: Read, T: serde::de::DeserializeOwned>(
    reader: &mut R,
) -> Result<T, FrameError> {
    let (kind, declared) = read_header(reader)?;
    if kind != KIND_MESSAGE {
        // The payload is still consumed, so the stream stays aligned and the
        // channel can survive a protocol slip rather than desynchronising.
        let _ = read_exact_payload(reader, declared);
        return Err(if kind == KIND_BLOB {
            FrameError::WrongKind { expected: "message", actual: "blob" }
        } else {
            FrameError::UnknownKind { kind }
        });
    }

    let payload = read_exact_payload(reader, declared)?;
    serde_json::from_slice(&payload).map_err(|error| FrameError::Malformed(error.to_string()))
}

/// Reads one raw byte payload.
pub fn read_blob<R: Read>(reader: &mut R) -> Result<Vec<u8>, FrameError> {
    let (kind, declared) = read_header(reader)?;
    if kind != KIND_BLOB {
        let _ = read_exact_payload(reader, declared);
        return Err(FrameError::WrongKind { expected: "blob", actual: "message" });
    }
    read_exact_payload(reader, declared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Request;

    #[test]
    fn frames_round_trip() {
        let request = Request::RenderPage { page: 3, zoom: 1.5, rotation: 90 };
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &request).unwrap();

        let decoded: Request = read_frame(&mut buffer.as_slice()).unwrap();
        assert_eq!(request, decoded);
    }

    #[test]
    fn multiple_frames_read_in_order() {
        let mut buffer = Vec::new();
        for page in 0..3 {
            write_frame(&mut buffer, &Request::ExtractText { page }).unwrap();
        }

        let mut cursor = buffer.as_slice();
        for expected in 0..3 {
            let decoded: Request = read_frame(&mut cursor).unwrap();
            assert_eq!(decoded, Request::ExtractText { page: expected });
        }
    }

    #[test]
    fn oversized_declaration_is_rejected_before_allocating() {
        // The attack: a compromised worker announces a huge frame and the host
        // allocates it. The length must be checked first.
        let mut frame = vec![0_u8];
        frame.extend_from_slice(&u32::MAX.to_be_bytes());
        frame.extend_from_slice(b"{}");

        let result: Result<Request, _> = read_frame(&mut frame.as_slice());
        assert!(matches!(result, Err(FrameError::TooLarge { .. })));
    }

    #[test]
    fn truncated_payload_reports_closed_not_malformed() {
        // Distinguishing "the worker died mid-write" from "the worker sent
        // garbage" matters: one triggers a restart, the other is a bug.
        let mut frame = vec![0_u8];
        frame.extend_from_slice(&100_u32.to_be_bytes());
        frame.extend_from_slice(b"partial");

        let result: Result<Request, _> = read_frame(&mut frame.as_slice());
        assert!(matches!(result, Err(FrameError::Closed)), "{result:?}");
    }

    #[test]
    fn blobs_round_trip_without_json_expansion() {
        // The whole point: a megabyte of pixels must cost a megabyte on the
        // wire, not 3.6 megabytes.
        let pixels: Vec<u8> = (0..1_000_000).map(|n| (n % 256) as u8).collect();

        let mut buffer = Vec::new();
        write_blob(&mut buffer, &pixels).unwrap();

        assert!(
            buffer.len() < pixels.len() + 16,
            "blob framing added {} bytes of overhead",
            buffer.len() - pixels.len()
        );

        let decoded = read_blob(&mut buffer.as_slice()).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn a_blob_read_as_a_message_is_refused_without_desynchronising() {
        // A protocol slip must not leave the stream misaligned: the next frame
        // has to still be readable, or one mistake corrupts the channel.
        let mut buffer = Vec::new();
        write_blob(&mut buffer, &[1, 2, 3]).unwrap();
        write_frame(&mut buffer, &Request::CloseDocument).unwrap();

        let mut cursor = buffer.as_slice();
        let wrong: Result<Request, _> = read_frame(&mut cursor);
        assert!(matches!(wrong, Err(FrameError::WrongKind { .. })), "{wrong:?}");

        // The following message is still intact.
        let next: Request = read_frame(&mut cursor).unwrap();
        assert_eq!(next, Request::CloseDocument);
    }

    #[test]
    fn a_message_read_as_a_blob_is_refused() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &Request::CloseDocument).unwrap();
        let result = read_blob(&mut buffer.as_slice());
        assert!(matches!(result, Err(FrameError::WrongKind { .. })), "{result:?}");
    }

    #[test]
    fn empty_channel_reports_closed() {
        let result: Result<Request, _> = read_frame(&mut [].as_slice());
        assert!(matches!(result, Err(FrameError::Closed)));
    }

    #[test]
    fn garbage_payload_reports_malformed() {
        let payload = b"not json at all";
        let mut frame = vec![0_u8];
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);

        let result: Result<Request, _> = read_frame(&mut frame.as_slice());
        assert!(matches!(result, Err(FrameError::Malformed(_))), "{result:?}");
    }
}
