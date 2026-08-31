//! Single-flight wrapper around the worker supervisor (Phase 3).
//!
//! Mirrors the sidecar runner's concurrency semantics:
//! - one flight at a time — every caller within a refresh window shares the
//!   same success or failure result;
//! - results (success *and* failure) are cached briefly
//!   ([`RESULT_FRESH_FOR`]) so a broken collector is not relaunched in a tight
//!   loop;
//! - the flight is never permanently stuck: a worker panic, timeout, or
//!   protocol error resolves the flight and later refreshes recover;
//! - application shutdown cancels the whole in-flight collection and blocks
//!   new flights.
//!
//! Since Phase 4B this is also the production data source: the Tauri command
//! and the tray refresh call [`collect_usage`], which submits ONE batch
//! snapshot request over the full product agent registry and folds the
//! response into the public `UsageSummary` through `normalize_snapshot`. The
//! production path never references the v0.2 sidecar runner.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::protocol::{CollectorRequestV1, DataSourceV1, DateWindowV1};
use super::snapshot_protocol::{
    AgentSpecV1, CollectorSnapshotRequestV1, SNAPSHOT_PROTOCOL_VERSION,
};
use super::supervisor;
use super::{AgentKind, CollectResult, CollectorError};
use crate::error::AppError;
use crate::usage::UsageSummary;

/// Short-lived result cache, identical to the sidecar runner's semantics:
/// caching failures as well as successes prevents a broken collector from
/// being relaunched in a tight loop while still allowing a prompt retry.
pub const RESULT_FRESH_FOR: Duration = Duration::from_secs(2);

/// State of the one allowed in-flight collection.
struct Flight {
    finished: bool,
    result: Result<CollectResult, CollectorError>,
}

static FLIGHT: Mutex<Option<Arc<(Mutex<Flight>, Condvar)>>> = Mutex::new(None);
static LAST_RESULT: Mutex<Option<CachedWorkerResult>> = Mutex::new(None);

struct CachedWorkerResult {
    created: Instant,
    result: Result<CollectResult, CollectorError>,
}

/// Runs one collection through the worker, sharing the flight with all
/// concurrent callers. `request` must carry the agent and source of the
/// desired collection; the shared flight uses the *first* caller's request.
pub fn collect(request: &CollectorRequestV1) -> Result<CollectResult, CollectorError> {
    if let Some(cached) = fresh_cached() {
        return cached;
    }

    // Claim the flight, or wait for the running one.
    let mut guard = FLIGHT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_some() {
        // Join the in-flight collection.
        let flight = Arc::clone(guard.as_ref().expect("flight present"));
        drop(guard);
        return wait_for_flight(&flight);
    }

    // First caller: run the supervisor while holding the flight slot.
    let flight = Arc::new((
        Mutex::new(Flight {
            finished: false,
            result: Err(CollectorError::Cancelled),
        }),
        Condvar::new(),
    ));
    *guard = Some(Arc::clone(&flight));
    drop(guard);

    let result = supervisor::collect(request);

    // Resolve the flight for all joiners, then clear the slot and cache.
    {
        let (flight_mutex, condvar) = &*flight;
        let mut state = flight_mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.finished = true;
        state.result = result.clone();
        condvar.notify_all();
    }
    let mut guard = FLIGHT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
    drop(guard);
    let mut last = LAST_RESULT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *last = Some(CachedWorkerResult {
        created: Instant::now(),
        result: result.clone(),
    });
    result
}

fn wait_for_flight(
    flight: &Arc<(Mutex<Flight>, Condvar)>,
) -> Result<CollectResult, CollectorError> {
    let (mutex, condvar) = &**flight;
    let mut guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    while !guard.finished {
        guard = condvar.wait(guard).expect("flight condvar wait");
    }
    guard.result.clone()
}

fn fresh_cached() -> Option<Result<CollectResult, CollectorError>> {
    let mut last = LAST_RESULT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(cached) = last.as_ref() {
        if cached.created.elapsed() < RESULT_FRESH_FOR {
            return Some(cached.result.clone());
        }
    }
    *last = None;
    None
}

/// True when this agent is exposed through the worker path (production since
/// Phase 4B).
pub fn supports_agent(agent: AgentKind) -> bool {
    let _ = agent;
    true
}

// --- Batch snapshot single-flight (Phase 4A) --------------------------------

static SNAPSHOT_FLIGHT: Mutex<Option<Arc<(Mutex<Option<SnapshotFlightResult>>, Condvar)>>> =
    Mutex::new(None);
static SNAPSHOT_LAST: Mutex<Option<CachedSnapshotResult>> = Mutex::new(None);

struct SnapshotFlightResult {
    result: Result<super::snapshot_protocol::CollectorSnapshotResponseV1, CollectorError>,
}

struct CachedSnapshotResult {
    created: Instant,
    result: Result<super::snapshot_protocol::CollectorSnapshotResponseV1, CollectorError>,
}

/// Runs one full-agent snapshot through the worker, sharing the flight with
/// all concurrent callers: 20 concurrent "full refresh" calls share ONE worker
/// process and one snapshot result. Success and failure are both cached for
/// [`RESULT_FRESH_FOR`], matching the sidecar runner.
pub fn collect_snapshot(
    request: &super::snapshot_protocol::CollectorSnapshotRequestV1,
) -> Result<super::snapshot_protocol::CollectorSnapshotResponseV1, CollectorError> {
    if let Some(cached) = fresh_snapshot_cache() {
        return cached;
    }

    let mut guard = SNAPSHOT_FLIGHT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(flight) = guard.as_ref() {
        let flight = Arc::clone(flight);
        drop(guard);
        return wait_for_snapshot_flight(&flight);
    }

    let flight = Arc::new((Mutex::new(None::<SnapshotFlightResult>), Condvar::new()));
    *guard = Some(Arc::clone(&flight));
    drop(guard);

    let result = supervisor::collect_snapshot(request);

    {
        let (mutex, condvar) = &*flight;
        let mut slot = mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(SnapshotFlightResult {
            result: result.clone(),
        });
        condvar.notify_all();
    }
    let mut guard = SNAPSHOT_FLIGHT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
    drop(guard);
    let mut last = SNAPSHOT_LAST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *last = Some(CachedSnapshotResult {
        created: Instant::now(),
        result: result.clone(),
    });
    result
}

fn fresh_snapshot_cache(
) -> Option<Result<super::snapshot_protocol::CollectorSnapshotResponseV1, CollectorError>> {
    let mut last = SNAPSHOT_LAST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(cached) = last.as_ref() {
        if cached.created.elapsed() < RESULT_FRESH_FOR {
            return Some(cached.result.clone());
        }
    }
    *last = None;
    None
}

fn wait_for_snapshot_flight(
    flight: &Arc<(Mutex<Option<SnapshotFlightResult>>, Condvar)>,
) -> Result<super::snapshot_protocol::CollectorSnapshotResponseV1, CollectorError> {
    let (mutex, condvar) = &**flight;
    let mut guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    while guard.is_none() {
        guard = condvar.wait(guard).expect("snapshot flight condvar wait");
    }
    guard.as_ref().expect("flight resolved").result.clone()
}

/// Test-only seam: clears the snapshot result cache so integration tests do
/// not observe each other's cached results across scenarios. Never called by
/// production code (the cache is process-global in the real app by design).
#[doc(hidden)]
pub fn clear_snapshot_result_cache_for_tests() {
    if let Ok(mut last) = SNAPSHOT_LAST.lock() {
        *last = None;
    }
}

// --- Production full refresh (Phase 4B) -------------------------------------

/// Width of the production refresh window: today-6 ..= today, exactly the
/// `--since` bound the v0.2 sidecar runner passed to ccusage.
const PROD_WINDOW_DAYS: i64 = 6;

/// Runs one full production refresh: a single batch snapshot worker over the
/// entire product agent registry, folded into the public `UsageSummary` by
/// `normalize_snapshot`.
///
/// Concurrency, caching and shutdown semantics are the runner's (identical to
/// the v0.2 sidecar runner): every caller within [`RESULT_FRESH_FOR`] shares
/// one worker and one result — success or failure — via `collect_snapshot`'s
/// single-flight; a new worker is refused once shutdown has begun.
///
/// No sidecar is looked up, spawned or fallen back to on this path. There is
/// deliberately only ONE cache in the production chain (the snapshot result
/// cache inside [`collect_snapshot`]); the `UsageSummary` adapter runs fresh
/// on every call (sub-millisecond) so no second cache layer exists.
pub fn collect_usage() -> Result<UsageSummary, AppError> {
    let response = collect_snapshot(&production_snapshot_request())?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let collected_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    crate::sidecar::adapter::normalize_snapshot(&response, &today, &collected_at)
}

/// The production batch request: every registered agent (deterministic
/// registry order) through its own environment-resolved data source, over the
/// seven-day window, bucketed in the system time zone.
///
/// Public for the production-path verification suite (request shape audit and
/// single-flight tests); it is a pure builder with no side effects.
pub fn production_snapshot_request() -> CollectorSnapshotRequestV1 {
    let today = chrono::Local::now().date_naive();
    let start = today - chrono::Duration::days(PROD_WINDOW_DAYS);
    CollectorSnapshotRequestV1 {
        version: SNAPSHOT_PROTOCOL_VERSION,
        request_id: format!("prod-{}", chrono::Utc::now().timestamp_millis()),
        agents: AgentKind::ALL
            .iter()
            .map(|agent| AgentSpecV1 {
                agent: agent.id().to_string(),
                source: DataSourceV1::Environment,
            })
            .collect(),
        window: Some(DateWindowV1 {
            start_inclusive: start.format("%Y-%m-%d").to_string(),
            end_inclusive: today.format("%Y-%m-%d").to_string(),
        }),
        timezone: product_timezone(),
    }
}

/// The system IANA time-zone name for daily bucketing. The v0.2 sidecar
/// bucketed by the ccusage process's local zone; the worker request pins the
/// same zone explicitly so the parent's `chrono::Local` `today` and the
/// engine's day buckets always agree.
///
/// `TimeZone::try_system` resolves via the OS (Windows registry + CLDR
/// mapping in jiff). The `"system"` marker only appears when no IANA name can
/// be produced; the vendored engine then falls back to its own system zone
/// (`ccusage-core/src/date_utils.rs` resolves the request zone through jiff
/// and uses the system zone when the name does not resolve), which is the
/// same zone the parent used for `today`.
fn product_timezone() -> String {
    match jiff::tz::TimeZone::try_system() {
        Ok(tz) => tz
            .iana_name()
            .map(str::to_string)
            .unwrap_or_else(|| "system".to_string()),
        Err(_) => "system".to_string(),
    }
}
