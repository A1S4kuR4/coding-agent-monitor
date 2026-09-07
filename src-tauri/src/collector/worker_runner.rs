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
use crate::usage::{UsageScope, UsageSummary};

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

static SNAPSHOT_FLIGHT: Mutex<Option<Arc<SnapshotFlight>>> = Mutex::new(None);
static SNAPSHOT_LAST: Mutex<Option<CachedSnapshotResult>> = Mutex::new(None);

struct SnapshotFlight {
    request: CollectorSnapshotRequestV1,
    result: Mutex<Option<SnapshotFlightResult>>,
    completed: Condvar,
}

struct SnapshotFlightResult {
    result: Result<super::snapshot_protocol::CollectorSnapshotResponseV1, CollectorError>,
}

struct CachedSnapshotResult {
    created: Instant,
    request: CollectorSnapshotRequestV1,
    result: Result<super::snapshot_protocol::CollectorSnapshotResponseV1, CollectorError>,
}

/// Runs one full-agent snapshot through the worker, sharing the flight with
/// all concurrent callers: 20 concurrent "full refresh" calls share ONE worker
/// process and one snapshot result. Success and failure are both cached for
/// [`RESULT_FRESH_FOR`], matching the sidecar runner.
pub fn collect_snapshot(
    request: &super::snapshot_protocol::CollectorSnapshotRequestV1,
) -> Result<super::snapshot_protocol::CollectorSnapshotResponseV1, CollectorError> {
    if let Some(cached) = fresh_snapshot_cache(request) {
        return cached;
    }

    let mut guard = SNAPSHOT_FLIGHT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(flight) = guard.as_ref() {
        let flight = Arc::clone(flight);
        let same_request = same_snapshot_query(&flight.request, request);
        drop(guard);
        let result = wait_for_snapshot_flight(&flight);
        // A new local date/time-zone must not join the old range. Wait for the
        // existing flight to release the single slot, then start the requested
        // range as a new flight.
        return if same_request {
            result
        } else {
            collect_snapshot(request)
        };
    }

    let flight = Arc::new(SnapshotFlight {
        request: request.clone(),
        result: Mutex::new(None::<SnapshotFlightResult>),
        completed: Condvar::new(),
    });
    *guard = Some(Arc::clone(&flight));
    drop(guard);

    let result = supervisor::collect_snapshot(request);

    {
        let mut slot = flight
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some(SnapshotFlightResult {
            result: result.clone(),
        });
    }
    let mut last = SNAPSHOT_LAST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *last = Some(CachedSnapshotResult {
        created: Instant::now(),
        request: request.clone(),
        result: result.clone(),
    });
    drop(last);
    let mut guard = SNAPSHOT_FLIGHT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
    drop(guard);
    // Different-range waiters recurse only after the old flight slot and its
    // cache entry are fully settled, avoiding a wake/retry race at midnight.
    flight.completed.notify_all();
    result
}

fn fresh_snapshot_cache(
    request: &CollectorSnapshotRequestV1,
) -> Option<Result<super::snapshot_protocol::CollectorSnapshotResponseV1, CollectorError>> {
    let mut last = SNAPSHOT_LAST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(cached) = last.as_ref() {
        if cached.created.elapsed() < RESULT_FRESH_FOR
            && same_snapshot_query(&cached.request, request)
        {
            return Some(cached.result.clone());
        }
    }
    *last = None;
    None
}

fn wait_for_snapshot_flight(
    flight: &Arc<SnapshotFlight>,
) -> Result<super::snapshot_protocol::CollectorSnapshotResponseV1, CollectorError> {
    let mut guard = flight
        .result
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    while guard.is_none() {
        guard = flight
            .completed
            .wait(guard)
            .expect("snapshot flight condvar wait");
    }
    guard.as_ref().expect("flight resolved").result.clone()
}

fn same_snapshot_query(
    left: &CollectorSnapshotRequestV1,
    right: &CollectorSnapshotRequestV1,
) -> bool {
    left.version == right.version
        && left.agents == right.agents
        && left.window == right.window
        && left.timezone == right.timezone
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
    let today = chrono::Local::now().date_naive();
    let scope = UsageScope {
        start_date: (today - chrono::Duration::days(PROD_WINDOW_DAYS))
            .format("%Y-%m-%d")
            .to_string(),
        end_date: today.format("%Y-%m-%d").to_string(),
        time_zone: crate::usage::state::system_time_zone(),
    };
    collect_usage_for_scope(&scope)
}

/// Runs the production worker for one already-captured local date/time-zone
/// scope. Capturing the scope before the worker starts prevents midnight from
/// relabelling an old-range response as a new day's successful snapshot.
pub fn collect_usage_for_scope(scope: &UsageScope) -> Result<UsageSummary, AppError> {
    let response = collect_snapshot(&production_snapshot_request_for_scope(scope))?;
    let collected_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    crate::sidecar::adapter::normalize_snapshot(&response, &scope.end_date, &collected_at)
}

/// The production batch request: every registered agent (deterministic
/// registry order) through its own environment-resolved data source, over the
/// seven-day window, bucketed in the system time zone.
///
/// Public for the production-path verification suite (request shape audit and
/// single-flight tests); it is a pure builder with no side effects.
pub fn production_snapshot_request() -> CollectorSnapshotRequestV1 {
    let today = chrono::Local::now().date_naive();
    let scope = UsageScope {
        start_date: (today - chrono::Duration::days(PROD_WINDOW_DAYS))
            .format("%Y-%m-%d")
            .to_string(),
        end_date: today.format("%Y-%m-%d").to_string(),
        time_zone: crate::usage::state::system_time_zone(),
    };
    production_snapshot_request_for_scope(&scope)
}

pub fn production_snapshot_request_for_scope(scope: &UsageScope) -> CollectorSnapshotRequestV1 {
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
            start_inclusive: scope.start_date.clone(),
            end_inclusive: scope.end_date.clone(),
        }),
        timezone: scope.time_zone.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_cache_identity_includes_range_and_time_zone_but_not_request_id() {
        let scope = UsageScope {
            start_date: "2026-08-31".into(),
            end_date: "2026-09-06".into(),
            time_zone: "Asia/Shanghai".into(),
        };
        let first = production_snapshot_request_for_scope(&scope);
        let mut same_query = first.clone();
        same_query.request_id = "another-id".into();
        assert!(same_snapshot_query(&first, &same_query));

        let mut next_day = same_query.clone();
        next_day.window.as_mut().unwrap().end_inclusive = "2026-09-07".into();
        assert!(!same_snapshot_query(&first, &next_day));

        let mut next_zone = same_query;
        next_zone.timezone = "UTC".into();
        assert!(!same_snapshot_query(&first, &next_zone));
    }
}
