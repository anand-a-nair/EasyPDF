//! Self-confinement applied at worker startup.
//!
//! The worker restricts itself **before reading a single byte of untrusted
//! input**. Ordering is the whole point: confinement applied after parsing
//! begins protects nothing.
//!
//! Two independent mechanisms, because they fail differently:
//!
//! - **OS confinement** (seatbelt on macOS) removes capabilities entirely —
//!   filesystem, network, subprocesses.
//! - **Resource limits** (`setrlimit`) cap memory and CPU, so a decompression
//!   bomb hits a ceiling even inside a confined process.
//!
//! Where confinement cannot be applied the worker says so rather than
//! pretending. See [`SandboxStatus`].

use easypdf_ffi::protocol::{ResourceLimits, SandboxStatus};

/// Address-space ceiling for the worker, in bytes.
///
/// Backstop for the accounting in `easypdf_ffi::limits`: if the parser's own
/// checks are bypassed by a bug, the kernel still refuses the allocation.
const MAX_ADDRESS_SPACE: u64 = 4 * 1024 * 1024 * 1024;

/// CPU-seconds before the kernel terminates the worker.
const MAX_CPU_SECONDS: u64 = 120;

/// Confines the current process as tightly as the platform allows.
///
/// Must be called before any untrusted input is read.
#[must_use]
pub(crate) fn apply() -> SandboxStatus {
    let resource_limits = apply_resource_limits();

    match apply_os_sandbox() {
        Ok(mechanism) => SandboxStatus::Enforced { mechanism, resource_limits },
        Err(reason) => SandboxStatus::NotEnforced { reason, resource_limits },
    }
}

#[cfg(unix)]
fn apply_resource_limits() -> ResourceLimits {
    #[allow(unsafe_code)]
    unsafe fn set(resource: libc::c_int, value: u64) -> bool {
        let limit = libc::rlimit { rlim_cur: value, rlim_max: value };
        // SAFETY: `limit` is a fully initialized rlimit and the pointer is
        // valid for the duration of the call. setrlimit only narrows this
        // process's own resource ceilings and cannot invalidate any Rust
        // invariant.
        unsafe { libc::setrlimit(resource, &raw const limit) == 0 }
    }

    // SAFETY: every call below is to the `set` helper above, whose own safety
    // requirements are documented and met — each passes a real resource
    // constant and a concrete value.
    #[allow(unsafe_code)]
    unsafe {
        // Never dump core: a crash dump of this process would contain the
        // document, which may be confidential.
        let core_dumps_disabled = set(libc::RLIMIT_CORE, 0);

        let cpu_seconds = set(libc::RLIMIT_CPU, MAX_CPU_SECONDS).then_some(MAX_CPU_SECONDS);

        // Verified on macOS 15 (arm64): both RLIMIT_AS and RLIMIT_DATA fail
        // with EINVAL regardless of whether the hard limit is also lowered.
        // The platform simply does not implement an address-space ceiling, so
        // this is reported as absent rather than quietly assumed.
        let address_space_bytes = (set(libc::RLIMIT_AS, MAX_ADDRESS_SPACE)
            || set(libc::RLIMIT_DATA, MAX_ADDRESS_SPACE))
        .then_some(MAX_ADDRESS_SPACE);

        ResourceLimits { core_dumps_disabled, cpu_seconds, address_space_bytes }
    }
}

#[cfg(not(unix))]
fn apply_resource_limits() -> ResourceLimits {
    // Windows equivalent is a job object with JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    // applied by the parent rather than by the child. Not yet implemented.
    ResourceLimits::none()
}

#[cfg(target_os = "macos")]
fn apply_os_sandbox() -> Result<String, String> {
    use std::ffi::{CString, c_char};

    // Seatbelt profile. `deny default` means every capability must be granted
    // explicitly; the worker needs almost none, because the host hands it an
    // already-open descriptor and it only computes.
    //
    // Already-open descriptors (stdin/stdout) survive confinement, which is
    // what makes such a tight profile workable.
    const PROFILE: &str = r#"
        (version 1)
        (deny default)
        (deny network*)
        (deny process-exec*)
        (deny file-write*)
        (allow file-read-metadata)
        (allow sysctl-read)
        (allow mach-lookup)
        (allow signal (target self))
    "#;

    // SAFETY: sandbox_init takes a NUL-terminated profile string and an out
    // pointer for an error buffer. Both are valid for the duration of the call.
    // The function only narrows this process's privileges.
    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> i32;
        fn sandbox_free_error(errorbuf: *mut c_char);
    }

    let profile = CString::new(PROFILE).map_err(|_| "profile contained a NUL byte".to_owned())?;
    let mut error: *mut c_char = std::ptr::null_mut();

    // SAFETY: see the declaration above. `profile` outlives the call, and
    // `error` is only read when the call reports failure.
    #[allow(unsafe_code)]
    let status = unsafe { sandbox_init(profile.as_ptr(), 0, &raw mut error) };

    if status == 0 {
        return Ok("seatbelt".to_owned());
    }

    // SAFETY: on failure sandbox_init sets `error` to a NUL-terminated string
    // that this process owns and must free with sandbox_free_error.
    #[allow(unsafe_code)]
    let message = unsafe {
        if error.is_null() {
            format!("sandbox_init failed with status {status}")
        } else {
            let text = std::ffi::CStr::from_ptr(error).to_string_lossy().into_owned();
            sandbox_free_error(error);
            text
        }
    };

    Err(message)
}

#[cfg(target_os = "linux")]
fn apply_os_sandbox() -> Result<String, String> {
    // Landlock (filesystem) plus seccomp-bpf (syscall filtering) is the right
    // pairing here. Not yet implemented — see ideas/08-open-questions.md.
    Err("linux confinement not yet implemented (landlock + seccomp-bpf planned)".to_owned())
}

#[cfg(target_os = "windows")]
fn apply_os_sandbox() -> Result<String, String> {
    // AppContainer with a job object, applied by the parent at spawn time.
    // Not yet implemented.
    Err("windows confinement not yet implemented (AppContainer planned)".to_owned())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn apply_os_sandbox() -> Result<String, String> {
    Err("no confinement mechanism known for this platform".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Confinement is process-global, so this is
    /// deliberately a single test. Splitting it caused the two halves to race
    /// and the second call to fail.
    #[test]
    fn confinement_applies_once_and_reports_honestly() {
        let status = apply();

        match &status {
            SandboxStatus::Enforced { mechanism, resource_limits } => {
                assert!(!mechanism.is_empty(), "enforced status must name its mechanism");
                assert!(resource_limits.core_dumps_disabled, "core dumps must be off");
                assert!(resource_limits.cpu_seconds.is_some(), "cpu ceiling must be set");
                // Memory ceiling is genuinely unavailable on macOS; asserting
                // it here would be asserting a platform lie.
                if cfg!(target_os = "macos") {
                    assert!(
                        !resource_limits.memory_capped(),
                        "macOS has no RLIMIT_AS — if this now succeeds, update the docs"
                    );
                }
            }
            SandboxStatus::NotEnforced { reason, .. } => {
                assert!(!reason.is_empty(), "an unconfined worker must explain why");
                // On macOS confinement is implemented, so failing here means
                // the seatbelt profile is wrong and the security model in
                // ideas/07-security.md is not holding.
                if cfg!(target_os = "macos") {
                    panic!("macOS confinement should have succeeded: {reason}");
                }
            }
            other => panic!("unhandled sandbox status: {other:?}"),
        }

        // Note: seatbelt permits layering an additional profile, so a second
        // sandbox_init succeeds. That is seatbelt's behavior, not ours, and is
        // deliberately not asserted on.
    }
}
