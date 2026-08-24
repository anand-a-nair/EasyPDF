//! Host-side supervision of the worker process.
//!
//! The host owns the worker's lifetime and trusts nothing it says. Three rules
//! shape this module, all following from `ideas/07-security.md`:
//!
//! 1. **A worker that misbehaves is killed, not debugged.** Timeouts and
//!    protocol violations end the process. A worker stuck on a pathological
//!    document does not recover, and one that has been exploited must not be
//!    given more input.
//! 2. **Every response is validated.** A compromised worker sends well-formed
//!    lies; deserializing successfully proves nothing.
//! 3. **Restart is a fresh process.** State is never carried across a death.

use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::framing::{FrameError, read_frame, write_frame};
use crate::protocol::{Request, ResourceLimits, Response, SandboxStatus, WorkerError};

/// Default deadline for a single request.
///
/// Generous, because a large page at high zoom legitimately takes time. The
/// worker enforces its own per-page limit; this is the outer backstop for a
/// worker that has stopped responding entirely.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// A running worker process.
pub struct Worker {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    responses: Receiver<Result<Response, FrameError>>,
    reader: Option<JoinHandle<()>>,
    executable: PathBuf,
    sandbox: SandboxStatus,
    timeout: Duration,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker")
            .field("pid", &self.child.id())
            .field("sandbox", &self.sandbox)
            .finish_non_exhaustive()
    }
}

impl Worker {
    /// Spawns a worker and completes the startup handshake.
    ///
    /// Returns once the worker has reported what confinement it applied. The
    /// caller should inspect [`Worker::sandbox`] — a worker running
    /// unconfined is a condition the user deserves to know about, not
    /// something to paper over.
    pub fn spawn(executable: &Path) -> Result<Self, WorkerError> {
        Self::spawn_with_timeout(executable, DEFAULT_TIMEOUT)
    }

    /// Spawns a worker with a custom request deadline.
    pub fn spawn_with_timeout(executable: &Path, timeout: Duration) -> Result<Self, WorkerError> {
        let mut child = Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // stderr is inherited so worker diagnostics reach the host's logs.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| WorkerError::Channel(format!("could not spawn worker: {error}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| WorkerError::Channel("worker stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| WorkerError::Channel("worker stdout unavailable".into()))?;

        // Reads happen on their own thread so the host can enforce a deadline.
        // A blocking read here would let a hung worker hang the whole UI.
        let (sender, responses) = channel();
        let reader = std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let frame = read_frame::<_, Response>(&mut stdout);
                let closed = frame.is_err();
                if sender.send(frame).is_err() || closed {
                    break;
                }
            }
        });

        let mut worker = Self {
            child,
            stdin: BufWriter::new(stdin),
            responses,
            reader: Some(reader),
            executable: executable.to_path_buf(),
            sandbox: SandboxStatus::NotEnforced {
                reason: "handshake not yet completed".into(),
                resource_limits: ResourceLimits::none(),
            },
            timeout,
        };

        match worker.request(&Request::Handshake)? {
            Response::Ready { version, sandbox } => {
                if version != env!("CARGO_PKG_VERSION") {
                    // A version mismatch means the protocol may differ. Refuse
                    // rather than guess.
                    worker.kill();
                    return Err(WorkerError::Channel(format!(
                        "worker version {version} does not match host {}",
                        env!("CARGO_PKG_VERSION")
                    )));
                }
                worker.sandbox = sandbox;
                Ok(worker)
            }
            other => {
                worker.kill();
                Err(WorkerError::Channel(format!("expected Ready handshake, got {other:?}")))
            }
        }
    }

    /// What confinement the worker applied to itself.
    #[must_use]
    pub fn sandbox(&self) -> &SandboxStatus {
        &self.sandbox
    }

    /// The worker's process id.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Sends a request and waits for the reply.
    ///
    /// On timeout or protocol violation the worker is killed. The caller gets
    /// an error and must spawn a fresh worker via [`Worker::restart`].
    pub fn request(&mut self, request: &Request) -> Result<Response, WorkerError> {
        if let Err(error) = write_frame(&mut self.stdin, request) {
            self.kill();
            return Err(match error {
                FrameError::Closed => WorkerError::WorkerDied,
                other => WorkerError::Channel(other.to_string()),
            });
        }

        let response = match self.responses.recv_timeout(self.timeout) {
            Ok(Ok(response)) => response,
            Ok(Err(FrameError::Closed)) => {
                self.kill();
                return Err(WorkerError::WorkerDied);
            }
            Ok(Err(error)) => {
                // A malformed frame means the worker is broken or hostile.
                self.kill();
                return Err(WorkerError::Channel(error.to_string()));
            }
            Err(RecvTimeoutError::Timeout) => {
                self.kill();
                return Err(WorkerError::Timeout {
                    timeout_ms: u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX),
                });
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.kill();
                return Err(WorkerError::WorkerDied);
            }
        };

        // Rule 2: never act on a response that contradicts itself.
        if !response.is_self_consistent() {
            self.kill();
            return Err(WorkerError::Channel(
                "worker returned a self-inconsistent response".into(),
            ));
        }

        Ok(response)
    }

    /// Kills this worker and returns a fresh one.
    ///
    /// Deliberately consuming: reusing a worker that has died — possibly while
    /// being exploited — is exactly the mistake this prevents at compile time.
    pub fn restart(mut self) -> Result<Self, WorkerError> {
        self.kill();
        let executable = self.executable.clone();
        let timeout = self.timeout;
        drop(self);
        Self::spawn_with_timeout(&executable, timeout)
    }

    /// Terminates the worker immediately.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Asks the worker to exit cleanly, killing it if it does not comply.
    pub fn shutdown(mut self) {
        let _ = write_frame(&mut self.stdin, &Request::Shutdown);
        // Give it a brief moment, then insist.
        std::thread::sleep(Duration::from_millis(50));
        self.kill();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // A worker outliving its handle would be a stray process holding a
        // document open.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
