//! Length-prefixed message framing for the worker channel.
//!
//! A 4-byte big-endian length followed by a JSON payload. JSON is not the
//! fastest choice, but the channel carries a handful of small control messages
//! per document — bulk pixel data is the exception and is measured before any
//! optimization happens here.
//!
//! **The frame limit is a security control, not a tuning parameter.** The host
//! reads frames from a process that may be compromised; without a ceiling, a
//! hostile worker announces a 4 GB frame and the host allocates it.

use std::io::{self, Read, Write};

/// Largest accepted frame, in bytes.
///
/// Sized to admit a full-page BGRA tile at high zoom with headroom. A worker
/// asking for more than this is malfunctioning or hostile; either way the host
/// must not honor it.
pub const MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;

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

    /// Underlying I/O failure.
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}

/// Writes one length-prefixed frame.
pub fn write_frame<W: Write, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), FrameError> {
    let payload =
        serde_json::to_vec(value).map_err(|error| FrameError::Malformed(error.to_string()))?;

    if payload.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge { declared: payload.len() });
    }

    // Cast is safe: the length was just bounded by MAX_FRAME_BYTES.
    let length = u32::try_from(payload.len())
        .map_err(|_| FrameError::TooLarge { declared: payload.len() })?;

    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Reads one length-prefixed frame.
///
/// The length is validated **before** any allocation, so a hostile declaration
/// costs nothing.
pub fn read_frame<R: Read, T: serde::de::DeserializeOwned>(
    reader: &mut R,
) -> Result<T, FrameError> {
    let mut length_bytes = [0_u8; 4];
    match reader.read_exact(&mut length_bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(FrameError::Closed);
        }
        Err(error) => return Err(FrameError::Io(error)),
    }

    let declared = u32::from_be_bytes(length_bytes) as usize;
    if declared > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge { declared });
    }

    let mut payload = vec![0_u8; declared];
    reader.read_exact(&mut payload).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            FrameError::Closed
        } else {
            FrameError::Io(error)
        }
    })?;

    serde_json::from_slice(&payload).map_err(|error| FrameError::Malformed(error.to_string()))
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
        let mut frame = u32::MAX.to_be_bytes().to_vec();
        frame.extend_from_slice(b"{}");

        let result: Result<Request, _> = read_frame(&mut frame.as_slice());
        assert!(matches!(result, Err(FrameError::TooLarge { .. })));
    }

    #[test]
    fn truncated_payload_reports_closed_not_malformed() {
        // Distinguishing "the worker died mid-write" from "the worker sent
        // garbage" matters: one triggers a restart, the other is a bug.
        let mut frame = 100_u32.to_be_bytes().to_vec();
        frame.extend_from_slice(b"partial");

        let result: Result<Request, _> = read_frame(&mut frame.as_slice());
        assert!(matches!(result, Err(FrameError::Closed)), "{result:?}");
    }

    #[test]
    fn empty_channel_reports_closed() {
        let result: Result<Request, _> = read_frame(&mut [].as_slice());
        assert!(matches!(result, Err(FrameError::Closed)));
    }

    #[test]
    fn garbage_payload_reports_malformed() {
        let payload = b"not json at all";
        let mut frame = (payload.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(payload);

        let result: Result<Request, _> = read_frame(&mut frame.as_slice());
        assert!(matches!(result, Err(FrameError::Malformed(_))), "{result:?}");
    }
}
