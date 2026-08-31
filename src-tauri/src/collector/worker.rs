//! Internal collector worker.
//!
//! The product executable doubles as the collector worker when launched with
//! [`INTERNAL_FLAG`] as the first argument. The flag is checked in `main`
//! before any Tauri/DB/tray initialization, so worker mode never touches the
//! UI stack, the app database, or the sidecar runner.
//!
//! Contract (see `docs/V0.3_PHASE3_WORKER.md`):
//! - stdin carries exactly one request JSON document — a single-agent
//!   `CollectorRequestV1` (the internal primitive) or a batch
//!   `CollectorSnapshotRequestV1` (the product refresh form) — terminated by
//!   EOF (trailing whitespace allowed, anything else rejected);
//! - stdout carries exactly one response JSON document — no banners, logs,
//!   progress, or panic text;
//! - diagnostics and debug output go to stderr (sanitized), which the parent
//!   reads with a byte cap;
//! - the process exits 0 when a response was written (success or structured
//!   error) and non-zero only when no response could be produced at all.
//!
//! This is an internal implementation detail, not a public CLI: the flag is
//! undocumented and the worker is not a security boundary.

use std::io::{Read, Write};

use super::protocol::{record_from_v1, CollectorRequestV1, CollectorResponseV1};
use super::snapshot_protocol::{
    AgentSnapshotOutcomeV1, AgentSnapshotV1, CollectorSnapshotRequestV1,
    CollectorSnapshotResponseV1, SNAPSHOT_PROTOCOL_VERSION,
};
use super::{ccusage::AgentCollector, AgentKind, Collector, CollectorError, DiagnosticKind};

/// The single internal argument that turns the product executable into a
/// collector worker. Never advertised anywhere user-visible.
pub const INTERNAL_FLAG: &str = "--cam-internal-collector-worker-v1";

/// Hard cap on stdin: a V1 request with 16 roots of 4 KiB paths is well under
/// 128 KiB; 256 KiB leaves generous headroom (mirrors the supervisor's write
/// cap so an over-limit request fails on the parent side first).
pub const MAX_STDIN_BYTES: usize = 256 * 1024;
/// Hard cap on stdout: a daily aggregate response for one agent is a few KiB;
/// 16 MiB accommodates pathological multi-year histories without allowing an
/// unbounded pipe write.
pub const MAX_STDOUT_BYTES: usize = 16 * 1024 * 1024;
/// Hard cap on stderr read by the parent (prevents a full pipe from deadlocking
/// both processes; beyond the cap stderr is truncated by the parent).
pub const MAX_STDERR_BYTES: usize = 64 * 1024;

/// True when the current process was started as a collector worker.
pub fn is_worker_invocation() -> bool {
    std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == INTERNAL_FLAG)
}

/// Runs the worker against real stdin/stdout. Returns the process exit code:
/// 0 when a response was written, non-zero when not even a response could be
/// produced.
pub fn run_worker_stdio() -> i32 {
    // Worker-only panic silencer: the parent learns about panics from the exit
    // path below, never from stderr panic text leaking into diagnostics. Set
    // here only — the normal Tauri entry point never reaches this function.
    std::panic::set_hook(Box::new(|_| {}));

    // Debug/test-only fault injection so integration tests can drive the
    // supervisor's failure paths through the real product binary. Compiled out
    // of release builds entirely.
    #[cfg(any(debug_assertions, test))]
    if let Err(exit_code) = run_debug_fault_injection() {
        return exit_code;
    }

    let mut stdin = std::io::stdin().lock();
    let mut input = Vec::new();
    // Read at most MAX+1 bytes: an over-limit request is detected by length,
    // not by trusting the pipe.
    let mut buffer = [0u8; 8192];
    loop {
        match stdin.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                input.extend_from_slice(&buffer[..n]);
                if input.len() > MAX_STDIN_BYTES {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_request_bytes(&input)
    }));
    let response_bytes = match outcome {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(failure)) => failure,
        Err(_panic) => error_response_bytes(&CollectorError::Internal {
            details: "collector worker panicked while handling the request".to_string(),
        }),
    };

    if response_bytes.len() > MAX_STDOUT_BYTES {
        // The success response itself exceeded the wire budget; replace it with
        // a small structured error rather than an unbounded write.
        let failure = error_response_bytes(&CollectorError::Internal {
            details: "collector response exceeded the stdout size limit".to_string(),
        });
        return write_once(&failure);
    }
    write_once(&response_bytes)
}

/// Writes exactly one response document to stdout and flushes. 0 = delivered.
fn write_once(response_bytes: &[u8]) -> i32 {
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(response_bytes).is_err() {
        return 2;
    }
    if stdout.flush().is_err() {
        return 2;
    }
    0
}

/// Builds the wire bytes of a structured error response.
fn error_response_bytes(error: &CollectorError) -> Vec<u8> {
    let response = CollectorResponseV1::error("worker-internal", error);
    serialize_response(&response)
}

/// Serializes a response document to wire bytes (with trailing newline). The
/// fallback keeps the "exactly one document" contract even if serialization of
/// our own types fails (which cannot happen).
fn serialize_response<T: serde::Serialize>(response: &T) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(response).unwrap_or_else(|_| {
        br#"{"version":1,"request_id":"worker-internal","status":"error","error":{"code":"internal","message":"collector worker failed"}}"#.to_vec()
    });
    bytes.push(b'\n');
    bytes
}

/// Parses and executes one request. Returns the wire bytes of the response to
/// write to stdout, or the bytes of a structured failure document when the
/// transport itself was invalid.
pub fn handle_request_bytes(input: &[u8]) -> Result<Vec<u8>, Vec<u8>> {
    let bytes = handle_request(input);
    if bytes.len() > MAX_STDOUT_BYTES {
        return Err(error_response_bytes(&CollectorError::Internal {
            details: "collector response exceeded the stdout size limit".to_string(),
        }));
    }
    Ok(bytes)
}

/// True when the payload is a batch snapshot request (has an `agents` array).
fn is_snapshot_request(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .is_some_and(|value| value.get("agents").is_some_and(serde_json::Value::is_array))
}

/// Executes one request and returns the response wire bytes. Snapshot requests
/// (with an `agents` array) collect every listed agent serially in-process in
/// deterministic registry order; single requests remain the internal
/// primitive.
fn handle_request(input: &[u8]) -> Vec<u8> {
    let transport_error =
        |details: String| error_response_bytes(&CollectorError::Protocol { details });

    if input.len() > MAX_STDIN_BYTES {
        return transport_error(format!(
            "request exceeds the {}-byte stdin limit ({} bytes received)",
            MAX_STDIN_BYTES,
            input.len()
        ));
    }
    let text = match std::str::from_utf8(input) {
        Ok(text) => text,
        Err(error) => {
            return transport_error(format!(
                "request is not valid UTF-8: {}",
                sanitized_utf8_error(error)
            ))
        }
    };
    if is_snapshot_request(text) {
        return handle_snapshot_request(text);
    }
    // serde rejects trailing content after the first JSON document (only
    // whitespace is tolerated), which enforces the one-request contract.
    let request: CollectorRequestV1 = match serde_json::from_str(text) {
        Ok(request) => request,
        Err(error) => {
            return transport_error(format!(
                "request is not exactly one CollectorRequestV1 document: {}",
                error
            ))
        }
    };
    let request_id = request.request_id.clone();
    match request.into_domain() {
        Ok(domain) => {
            let collector = AgentCollector::new(domain.agent);
            match Collector::collect(&collector, &domain) {
                Ok(result) => serialize_response(&CollectorResponseV1::ok(request_id, &result)),
                Err(error) => serialize_response(&CollectorResponseV1::error(request_id, &error)),
            }
        }
        Err(error) => serialize_response(&CollectorResponseV1::error(request_id, &error)),
    }
}

/// Executes a batch snapshot request: every listed agent is collected
/// serially in the worker process (deterministic registry order).
///
/// Per-agent failure policy:
/// - **`CollectorError`** (recoverable): the agent records a structured error
///   and the remaining agents continue — one agent's missing/corrupt data
///   does not prevent reading other agents.
/// - **Rust panic**: the **entire batch fails immediately**. The vendor
///   adapters share process-global state (load_context stores, the pricing
///   map, the load mutex) whose unwind safety cannot be guaranteed for all
///   upstream code paths. After a panic, partial results must never be
///   trusted or emitted. The partial agent_snapshots are discarded, a
///   whole-worker fatal `Internal` error response is emitted instead, and the
///   worker exits. The parent supervisor observes the error response and
///   does not cache it, so the next refresh starts clean.
fn handle_snapshot_request(text: &str) -> Vec<u8> {
    let transport_error =
        |details: String| error_response_bytes(&CollectorError::Protocol { details });

    let request: CollectorSnapshotRequestV1 = match serde_json::from_str(text) {
        Ok(request) => request,
        Err(error) => {
            return transport_error(format!(
                "request is not exactly one CollectorSnapshotRequestV1 document: {}",
                error
            ))
        }
    };
    let request_id = request.request_id.clone();
    let requests = match request.into_domain() {
        Ok(requests) => requests,
        Err(error) => return serialize_response(&CollectorResponseV1::error(request_id, &error)),
    };

    let mut agent_snapshots = Vec::with_capacity(requests.len());
    for domain in requests {
        let agent = domain.agent;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let collector = AgentCollector::new(agent);
            Collector::collect(&collector, &domain)
        }));
        let snapshot_outcome = match outcome {
            Ok(Ok(result)) => {
                let response = CollectorResponseV1::ok("internal", &result);
                match response.outcome {
                    crate::collector::protocol::OutcomeV1::Ok { report } => {
                        AgentSnapshotOutcomeV1::Ok { report }
                    }
                    crate::collector::protocol::OutcomeV1::Error { error } => {
                        AgentSnapshotOutcomeV1::Error { error }
                    }
                }
            }
            Ok(Err(error)) => {
                let response = CollectorResponseV1::error("internal", &error);
                match response.outcome {
                    crate::collector::protocol::OutcomeV1::Error { error } => {
                        AgentSnapshotOutcomeV1::Error { error }
                    }
                    crate::collector::protocol::OutcomeV1::Ok { .. } => {
                        unreachable!("error() always errors")
                    }
                }
            }
            Err(_panic) => {
                // Panic policy: fail the WHOLE batch. The vendor adapters
                // share process-global state (load_context, pricing map,
                // load mutex) whose unwind safety is not guaranteed. A panic
                // means that state may be corrupted; continuing would risk
                // returning wrong data. Discard all partial results, emit a
                // whole-worker fatal error, and let the worker exit.
                //
                // We return a valid CollectorSnapshotResponseV1 with
                // `fatal_error` set so the supervisor correctly maps this to
                // `Internal` — never `Protocol` or a partial result.
                return serialize_response(&CollectorSnapshotResponseV1 {
                    version: SNAPSHOT_PROTOCOL_VERSION,
                    request_id: request_id.clone(),
                    fatal_error: Some(crate::collector::protocol::ErrorV1 {
                        code: crate::collector::protocol::ErrorCodeV1::Internal,
                        message: format!(
                            "collector worker panicked while reading {} usage; \
                                 entire snapshot aborted to prevent untrustworthy results",
                            agent.label()
                        ),
                        agent: Some(agent.id().to_string()),
                        vendor: Some("ccusage v20.0.20".to_string()),
                    }),
                    agents: Vec::new(),
                });
            }
        };
        agent_snapshots.push(AgentSnapshotV1 {
            agent: agent.id().to_string(),
            outcome: snapshot_outcome,
        });
    }

    serialize_response(&CollectorSnapshotResponseV1 {
        version: SNAPSHOT_PROTOCOL_VERSION,
        request_id,
        fatal_error: None,
        agents: agent_snapshots,
    })
}

/// Converts a wire report back into the domain result (used by the supervisor).
pub fn result_from_wire_report(
    agent: AgentKind,
    report: &super::protocol::ReportV1,
) -> Result<super::CollectResult, CollectorError> {
    let mut records = Vec::with_capacity(report.records.len());
    for record in &report.records {
        records.push(record_from_v1(record)?);
    }
    let mut diagnostics = Vec::with_capacity(report.diagnostics.len());
    for diagnostic in &report.diagnostics {
        let kind = match diagnostic.kind.as_str() {
            "corrupt_file" => DiagnosticKind::CorruptFile,
            "corrupt_record" => DiagnosticKind::CorruptRecord,
            "database_error" => DiagnosticKind::DatabaseError,
            "source_unreadable" => DiagnosticKind::SourceUnreadable,
            "invariant_violation" => DiagnosticKind::InvariantViolation,
            "source_changed" => DiagnosticKind::SourceChanged,
            other => {
                return Err(CollectorError::Protocol {
                    details: format!("unknown diagnostic kind {other:?}"),
                })
            }
        };
        diagnostics.push(super::CollectionDiagnostic {
            kind,
            file: diagnostic.file.clone(),
            details: diagnostic.details.clone(),
        });
    }
    Ok(super::CollectResult::from_parts(
        agent,
        records,
        diagnostics,
    ))
}

/// A short, path-free description of a UTF-8 failure.
fn sanitized_utf8_error(error: std::str::Utf8Error) -> String {
    match error.error_len() {
        Some(len) => format!(
            "invalid byte sequence of {} byte(s) near offset {}",
            len,
            error.valid_up_to()
        ),
        None => format!(
            "incomplete byte sequence near offset {}",
            error.valid_up_to()
        ),
    }
}

/// Debug/test-only fault injection. Reads `CAM_TEST_WORKER_*` environment
/// variables; compiled out of release builds so a release EXE never embeds the
/// triggers. Returns `Err(exit_code)` when a fault fired and the worker should
/// stop before handling the request.
#[cfg(any(debug_assertions, test))]
fn run_debug_fault_injection() -> Result<(), i32> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static ARMED: AtomicU32 = AtomicU32::new(0);
    // Arm once so a recursion attempt (worker spawning a worker) cannot re-use
    // the injection flags in a child.
    if ARMED.fetch_add(1, Ordering::SeqCst) > 0 {
        return Ok(());
    }
    let enabled = |name: &str| std::env::var(name).is_ok();
    if enabled("CAM_TEST_WORKER_PANIC") {
        panic!("CAM_TEST_WORKER_PANIC fault injection (before any response is written)");
    }
    if let Ok(code) = std::env::var("CAM_TEST_WORKER_EXIT") {
        if let Ok(code) = code.parse::<i32>() {
            return Err(code);
        }
    }
    if enabled("CAM_TEST_WORKER_STDERR_FLOOD") {
        let line = "cam-worker-fault stderr flood line\n".repeat(2048);
        let mut stderr = std::io::stderr().lock();
        for _ in 0..8 {
            let _ = stderr.write_all(line.as_bytes());
        }
        let _ = stderr.flush();
    }
    if enabled("CAM_TEST_WORKER_SPAWN_MARKER") {
        if let Ok(path) = std::env::var("CAM_TEST_WORKER_SPAWN_MARKER") {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(file, "{}", std::process::id());
            }
        }
    }
    if enabled("CAM_TEST_WORKER_GARBAGE_STDOUT") {
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(b"this is not a collector response\n");
        let _ = stdout.flush();
        return Err(0);
    }
    if let Ok(ms) = std::env::var("CAM_TEST_WORKER_SLEEP_MS") {
        if let Ok(ms) = ms.parse::<u64>() {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }
    Ok(())
}
