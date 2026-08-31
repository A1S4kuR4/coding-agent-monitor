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
//! This runner is NOT the production data source yet — the Tauri command keeps
//! using the sidecar until Phase 4 shadow + switch.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::protocol::CollectorRequestV1;
use super::supervisor;
use super::{AgentKind, CollectResult, CollectorError};

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

/// True when this agent is exposed through the worker path (Phase 3 internal
/// enablement; the production Tauri command still uses the sidecar).
pub fn supports_agent(agent: AgentKind) -> bool {
    let _ = agent;
    true
}
