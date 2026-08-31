//! Shared application-shutdown flag.
//!
//! Both the v0.2 sidecar runner and the v0.3 collector worker supervisor gate
//! on this flag: once the app begins exiting, no new collector child may
//! start, and in-flight children are cancelled (kill + wait) by their own
//! supervisor. The flag lives here — not in either runner — so the production
//! worker path (Tauri command → worker_runner → supervisor) never needs to
//! reference the sidecar module.

use std::sync::atomic::{AtomicBool, Ordering};

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Marks the app as exiting. New collector children (sidecar or worker) are
/// refused from this point on; in-flight children observe the flag on their
/// supervisor's poll loop and are killed + reaped there.
pub fn begin_shutdown() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

/// True once the app has begun exiting.
pub fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}
