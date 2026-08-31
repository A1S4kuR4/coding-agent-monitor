//! Parent-side supervisor for the internal collector worker (Phase 3).
//!
//! Spawns the product executable (`current_exe()`) with the single internal
//! worker flag, feeds it one `CollectorRequestV1` over stdin, reads one
//! `CollectorResponseV1` from stdout (concurrently with a capped stderr drain),
//! validates the envelope, and converts the report back into CAM domain types.
//!
//! Failure-path contract:
//! - timeout and cancellation **kill + wait** the child on every path — the
//!   child never outlives the supervisor call;
//! - stdout is capped at `worker::MAX_STDOUT_BYTES`, stderr at
//!   `worker::MAX_STDERR_BYTES` (a flood is truncated, never allowed to
//!   deadlock the pipe);
//! - non-zero exit, missing output, malformed/oversize output, wrong protocol
//!   version and request-id mismatches map to stable `CollectorError`s;
//! - worker stderr is never surfaced to the UI or persisted — it is drained
//!   and dropped (its content is debug-only and may contain local paths);
//! - the worker is not a security boundary: it runs with the same privileges
//!   as the app and no permission checks cross the pipe.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::protocol::{CollectorRequestV1, CollectorResponseV1, OutcomeV1, PROTOCOL_VERSION};
use super::worker::{result_from_wire_report, INTERNAL_FLAG, MAX_STDERR_BYTES, MAX_STDOUT_BYTES};
use super::{AgentKind, CollectResult, CollectorError};

/// Default production budget for one worker collection. The tray refresh runs
/// every few minutes; a stuck worker must not occupy a whole refresh window.
pub const DEFAULT_WORKER_TIMEOUT: Duration = Duration::from_secs(120);

/// No-cancellation handle for `collect`.
static NEVER_CANCEL: AtomicBool = AtomicBool::new(false);

/// Worker executable override. Production always uses `current_exe()`; the
/// override exists ONLY so integration tests can point the supervisor at the
/// real product binary (a test harness's `current_exe()` would be the test
/// runner, never a worker). Not settable in release builds.
#[cfg(any(debug_assertions, test))]
static EXE_OVERRIDE: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

#[cfg(any(debug_assertions, test))]
pub fn set_worker_exe_override(path: std::path::PathBuf) {
    *EXE_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(path);
}

#[cfg(any(debug_assertions, test))]
fn worker_exe() -> std::io::Result<std::path::PathBuf> {
    if let Some(path) = EXE_OVERRIDE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        return Ok(path);
    }
    std::env::current_exe()
}

#[cfg(not(any(debug_assertions, test)))]
fn worker_exe() -> std::io::Result<std::path::PathBuf> {
    std::env::current_exe()
}

/// How the supervisor's interaction with the child ended.
enum ChildOutcome {
    Exited(std::process::ExitStatus),
    TimedOut,
    Cancelled,
}

/// Runs one collection in a worker child process.
pub fn collect(request: &CollectorRequestV1) -> Result<CollectResult, CollectorError> {
    collect_with_cancel(request, &NEVER_CANCEL)
}

/// Like [`collect`], but aborts (kill + wait) as soon as `cancel` flips to
/// true, mapping the outcome to [`CollectorError::Cancelled`].
pub fn collect_with_cancel(
    request: &CollectorRequestV1,
    cancel: &AtomicBool,
) -> Result<CollectResult, CollectorError> {
    collect_with_options(request, cancel, DEFAULT_WORKER_TIMEOUT)
}

/// Test/injectable seam: explicit timeout, for deterministic timeout tests
/// without long sleeps.
pub fn collect_with_options(
    request: &CollectorRequestV1,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<CollectResult, CollectorError> {
    let protocol_error = |details: String| CollectorError::Protocol { details };
    if crate::sidecar::is_shutting_down() || cancel.load(Ordering::SeqCst) {
        return Err(CollectorError::Cancelled);
    }

    let request_bytes = serde_json::to_vec(request)
        .map_err(|error| protocol_error(format!("failed to serialize request: {error}")))?;

    let (mut child, stdin, stdout, stderr) = spawn_worker()?;

    // Feed the request and close stdin immediately: the worker reads to EOF.
    let write_result = {
        let mut stdin = stdin;
        let result = stdin.write_all(&request_bytes).and_then(|_| stdin.flush());
        // Dropping stdin closes the pipe even on write failure.
        result
    };
    if let Err(error) = write_result {
        let _ = kill_and_wait(&mut child);
        return Err(CollectorError::Protocol {
            details: format!("failed to write request to worker stdin: {error}"),
        });
    }

    // Concurrent stdout/stderr drains so neither pipe can fill and block the
    // child. Both are capped; overflow beyond the cap is remembered and fails
    // validation below.
    let stdout_handle = std::thread::spawn(move || read_capped(stdout, MAX_STDOUT_BYTES));
    let stderr_handle = std::thread::spawn(move || read_capped(stderr, MAX_STDERR_BYTES));

    // Wait loop: bounded by `timeout`, interrupted by `cancel`.
    let deadline = Instant::now() + timeout;
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break ChildOutcome::Exited(status),
            Ok(None) => {}
            Err(error) => {
                let _ = kill_and_wait(&mut child);
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(CollectorError::Internal {
                    details: format!("failed to wait for worker: {error}"),
                });
            }
        }
        if cancel.load(Ordering::SeqCst) || crate::sidecar::is_shutting_down() {
            break ChildOutcome::Cancelled;
        }
        if Instant::now() >= deadline {
            break ChildOutcome::TimedOut;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    match outcome {
        ChildOutcome::TimedOut => {
            let _ = kill_and_wait(&mut child);
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            Err(CollectorError::Timeout {
                details: format!("worker did not finish within {}s", timeout.as_secs()),
            })
        }
        ChildOutcome::Cancelled => {
            let _ = kill_and_wait(&mut child);
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            Err(CollectorError::Cancelled)
        }
        ChildOutcome::Exited(status) => {
            let (stdout, stdout_overflowed) =
                stdout_handle.join().unwrap_or_else(|_| (Vec::new(), false));
            // stderr is drained and dropped: its content is debug-only.
            let _ = stderr_handle.join();

            if !status.success() {
                return Err(CollectorError::Internal {
                    details: format!(
                        "collector worker exited with {} (no response delivered)",
                        exit_code_text(&status)
                    ),
                });
            }
            if stdout_overflowed {
                return Err(protocol_error(
                    "worker stdout exceeded the response size limit".to_string(),
                ));
            }
            if stdout.is_empty() {
                return Err(CollectorError::Internal {
                    details: "collector worker produced no output".to_string(),
                });
            }
            let text = std::str::from_utf8(&stdout)
                .map_err(|_| protocol_error("worker stdout is not valid UTF-8".to_string()))?;
            let response = CollectorResponseV1::from_wire(text)?;
            if response.version != PROTOCOL_VERSION {
                return Err(protocol_error(format!(
                    "worker response version {} (expected {PROTOCOL_VERSION})",
                    response.version
                )));
            }
            if response.request_id != request.request_id {
                return Err(protocol_error(
                    "worker response request_id does not match the request".to_string(),
                ));
            }
            match &response.outcome {
                OutcomeV1::Ok { report } => {
                    let agent = AgentKind::from_id(&request.agent).ok_or_else(|| {
                        protocol_error(format!("unknown agent id {:?}", request.agent))
                    })?;
                    result_from_wire_report(agent, report)
                }
                OutcomeV1::Error { error } => Ok(Err(CollectorError::Protocol {
                    details: error.message.clone(),
                })?),
            }
        }
    }
}

fn spawn_worker() -> Result<(Child, ChildStdin, ChildStdout, ChildStderr), CollectorError> {
    let exe = worker_exe().map_err(|error| CollectorError::Internal {
        details: format!("failed to locate the product executable: {error}"),
    })?;
    let mut command = Command::new(exe);
    command
        .arg(INTERNAL_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Windows: never flash a console window for the worker.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|error| CollectorError::Internal {
        details: format!("failed to spawn collector worker: {error}"),
    })?;
    let stdin = child.stdin.take().expect("worker stdin piped");
    let stdout = child.stdout.take().expect("worker stdout piped");
    let stderr = child.stderr.take().expect("worker stderr piped");
    Ok((child, stdin, stdout, stderr))
}

type ChildStdin = std::process::ChildStdin;
type ChildStdout = std::process::ChildStdout;
type ChildStderr = std::process::ChildStderr;

/// Kills the child and waits for it. Called on every non-clean path so no
/// worker process or handle survives the supervisor.
fn kill_and_wait(child: &mut Child) -> std::io::Result<std::process::ExitStatus> {
    let _ = child.kill();
    child.wait()
}

fn exit_code_text(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => "an abnormal termination".to_string(),
    }
}

/// Reads a pipe up to `cap` bytes. Returns (bytes, overflowed): overflow means
/// the peer wrote past the cap (the excess is read and discarded so the pipe
/// still drains and the child cannot block forever).
fn read_capped<R: Read>(mut pipe: R, cap: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut overflowed = false;
    let mut buffer = [0u8; 8192];
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if out.len() < cap {
                    let take = n.min(cap - out.len());
                    out.extend_from_slice(&buffer[..take]);
                }
                if out.len() >= cap {
                    // Keep draining without storing so the writer never blocks.
                    overflowed = true;
                }
            }
            Err(_) => break,
        }
    }
    (out, overflowed)
}
