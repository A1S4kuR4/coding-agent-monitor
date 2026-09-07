use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::{collector::worker_runner, error::AppError};

use super::snapshot_store::{NoopSnapshotStore, SnapshotStore};
use super::UsageSummary;

/// The tray refreshes every five minutes. Ten minutes leaves room for one
/// delayed/failed cycle without labelling normally refreshed data as stale.
pub const STALE_AFTER: Duration = Duration::from_secs(10 * 60);
const RESULT_FRESH_FOR: Duration = worker_runner::RESULT_FRESH_FOR;
const WINDOW_DAYS: i64 = 6;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageScope {
    pub start_date: String,
    pub end_date: String,
    pub time_zone: String,
}

impl UsageScope {
    fn from_reading(reading: &ClockReading) -> Self {
        Self {
            start_date: (reading.local_date - chrono::Duration::days(WINDOW_DAYS))
                .format("%Y-%m-%d")
                .to_string(),
            end_date: reading.local_date.format("%Y-%m-%d").to_string(),
            time_zone: reading.time_zone.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshTrigger {
    Startup,
    Manual,
    Focus,
    Tray,
    Periodic,
    Recovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshOutcome {
    InProgress,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshFailureKind {
    TimedOut,
    Cancelled,
    Failed,
}

impl RefreshFailureKind {
    fn from_error(error: &AppError) -> Self {
        match error.code.as_str() {
            "collection_timeout" => Self::TimedOut,
            "collection_cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAttempt {
    /// Process-local monotonic identity. `u32` is always a safe JS integer.
    pub id: u32,
    pub trigger: RefreshTrigger,
    pub scope: UsageScope,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub outcome: RefreshOutcome,
    pub failure: Option<RefreshFailureKind>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub scope: UsageScope,
    pub summary: UsageSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FreshnessStatus {
    Fresh,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FreshnessReason {
    Current,
    NoSnapshot,
    Expired,
    DateChanged,
    TimeZoneChanged,
    ClockSkew,
    InvalidTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreshnessInfo {
    pub status: FreshnessStatus,
    pub reason: FreshnessReason,
    pub checked_at: String,
    pub current_date: String,
    pub current_time_zone: String,
    pub stale_after_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCollectionState {
    /// Process-local monotonic transition number. `u32` is a JS-safe integer.
    pub revision: u32,
    pub refreshing: bool,
    pub snapshot: Option<UsageSnapshot>,
    pub last_attempt: Option<RefreshAttempt>,
    pub freshness: FreshnessInfo,
}

#[derive(Debug, Clone)]
struct ClockReading {
    now_utc: DateTime<Utc>,
    local_date: NaiveDate,
    time_zone: String,
}

trait StateClock: Send + Sync {
    fn read(&self) -> ClockReading;
}

struct SystemClock;

impl StateClock for SystemClock {
    fn read(&self) -> ClockReading {
        ClockReading {
            now_utc: Utc::now(),
            local_date: chrono::Local::now().date_naive(),
            time_zone: system_time_zone(),
        }
    }
}

trait UsageCollector: Send + Sync {
    fn collect(&self, scope: &UsageScope) -> Result<UsageSummary, AppError>;
}

struct ProductionCollector;

impl UsageCollector for ProductionCollector {
    fn collect(&self, scope: &UsageScope) -> Result<UsageSummary, AppError> {
        worker_runner::collect_usage_for_scope(scope)
    }
}

#[derive(Default)]
struct RuntimeState {
    revision: u32,
    next_attempt_id: u32,
    refreshing: bool,
    snapshot: Option<UsageSnapshot>,
    last_attempt: Option<RefreshAttempt>,
    last_finished_at: Option<DateTime<Utc>>,
}

/// One in-memory source of truth for command, event, dashboard and tray state.
/// The coordinator additionally owns the cross-restart last-success cache
/// (T06): a successful refresh persists the new snapshot, and startup can
/// restore the previous one without rewriting any of its timestamps.
pub struct CollectionCoordinator {
    runtime: Mutex<RuntimeState>,
    completed: Condvar,
    clock: Arc<dyn StateClock>,
    collector: Arc<dyn UsageCollector>,
    store: Arc<dyn SnapshotStore>,
}

impl Default for CollectionCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl CollectionCoordinator {
    /// Filesystem-free constructor. The app setup wires the production
    /// [`super::snapshot_store::FileSnapshotStore`] through [`Self::with_store`]
    /// before any command or refresh can run.
    pub fn new() -> Self {
        Self::with_store(Arc::new(NoopSnapshotStore))
    }

    /// Production constructor with the cross-restart cache store.
    pub fn with_store(store: Arc<dyn SnapshotStore>) -> Self {
        Self {
            runtime: Mutex::new(RuntimeState::default()),
            completed: Condvar::new(),
            clock: Arc::new(SystemClock),
            collector: Arc::new(ProductionCollector),
            store,
        }
    }

    /// Restores the persisted last-success snapshot (T06) into the empty
    /// runtime state so a restart can display the previous data immediately,
    /// before the first background refresh completes. The stored
    /// `collected_at`, data dates and time zone are never rewritten: every
    /// read recomputes freshness against the current clock, so a restored
    /// cache surfaces as stale/unknown under its original date exactly like
    /// the equivalent in-memory snapshot would. A missing, corrupt or
    /// unknown-version cache is simply "no cache" and never blocks startup.
    pub fn restore_from_store(&self) {
        let mut runtime = self
            .runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if runtime.snapshot.is_some() || runtime.refreshing {
            return;
        }
        if let Some(snapshot) = self.store.load() {
            runtime.revision = runtime.revision.saturating_add(1);
            runtime.snapshot = Some(snapshot);
        }
    }

    pub fn current_state(&self) -> UsageCollectionState {
        let reading = self.clock.read();
        let runtime = self
            .runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state_from_runtime(&runtime, &reading)
    }

    /// Starts or joins the one stateful refresh flight. Every transition is
    /// published through `on_change`; callers joining a flight receive the
    /// final shared state and never create a second attempt.
    pub fn refresh<F>(&self, trigger: RefreshTrigger, on_change: F) -> UsageCollectionState
    where
        F: Fn(&UsageCollectionState),
    {
        let mut requested_trigger = trigger;
        // A date/time-zone change during collection gets one immediate bounded
        // recovery pass. This covers midnight and sleep resume without polling.
        for pass in 0..2 {
            let (attempt_id, scope, started_state) = {
                let mut runtime = self
                    .runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());

                while runtime.refreshing {
                    runtime = self
                        .completed
                        .wait(runtime)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }

                let reading = self.clock.read();
                let scope = UsageScope::from_reading(&reading);
                if cached_state_is_fresh(&runtime, &reading, &scope) {
                    return state_from_runtime(&runtime, &reading);
                }

                runtime.next_attempt_id = runtime.next_attempt_id.saturating_add(1);
                runtime.revision = runtime.revision.saturating_add(1);
                runtime.refreshing = true;
                let attempt_id = runtime.next_attempt_id;
                runtime.last_attempt = Some(RefreshAttempt {
                    id: attempt_id,
                    trigger: requested_trigger,
                    scope: scope.clone(),
                    started_at: timestamp(reading.now_utc),
                    finished_at: None,
                    outcome: RefreshOutcome::InProgress,
                    failure: None,
                });
                (attempt_id, scope, state_from_runtime(&runtime, &reading))
            };
            on_change(&started_state);

            let result = self.collector.collect(&scope);
            let finished = self.clock.read();
            let finished_scope = UsageScope::from_reading(&finished);
            let mut persisted: Option<UsageSnapshot> = None;
            let final_state = {
                let mut runtime = self
                    .runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                runtime.refreshing = false;
                runtime.revision = runtime.revision.saturating_add(1);
                runtime.last_finished_at = Some(finished.now_utc);

                let mut failure = None;
                match result {
                    Ok(summary) if summary.today.date == scope.end_date => {
                        let candidate = UsageSnapshot {
                            scope: scope.clone(),
                            summary,
                        };
                        if should_replace_snapshot(
                            runtime.snapshot.as_ref(),
                            &candidate,
                            &finished_scope,
                        ) {
                            runtime.snapshot = Some(candidate.clone());
                            persisted = Some(candidate);
                        }
                    }
                    Ok(_) => failure = Some(RefreshFailureKind::Failed),
                    Err(error) => failure = Some(RefreshFailureKind::from_error(&error)),
                }

                if let Some(attempt) = runtime.last_attempt.as_mut() {
                    if attempt.id == attempt_id {
                        attempt.finished_at = Some(timestamp(finished.now_utc));
                        attempt.outcome = if failure.is_some() {
                            RefreshOutcome::Failed
                        } else {
                            RefreshOutcome::Succeeded
                        };
                        attempt.failure = failure;
                    }
                }
                let state = state_from_runtime(&runtime, &finished);
                self.completed.notify_all();
                state
            };
            // Persist outside the state lock (T06): a slow disk never blocks
            // readers, and a storage failure cannot touch the in-memory
            // success. Only a replaced snapshot is saved, so a failed or
            // superseded collection never overwrites the last success.
            if let Some(snapshot) = persisted {
                let _ = self.store.save(&snapshot);
            }
            on_change(&final_state);

            if pass == 0 && scope != finished_scope {
                requested_trigger = RefreshTrigger::Recovery;
                continue;
            }
            return final_state;
        }
        unreachable!("bounded refresh loop always returns")
    }

    #[cfg(test)]
    fn with_dependencies(clock: Arc<dyn StateClock>, collector: Arc<dyn UsageCollector>) -> Self {
        Self::with_parts(clock, collector, Arc::new(NoopSnapshotStore))
    }

    #[cfg(test)]
    fn with_parts(
        clock: Arc<dyn StateClock>,
        collector: Arc<dyn UsageCollector>,
        store: Arc<dyn SnapshotStore>,
    ) -> Self {
        Self {
            runtime: Mutex::new(RuntimeState::default()),
            completed: Condvar::new(),
            clock,
            collector,
            store,
        }
    }
}

fn cached_state_is_fresh(
    runtime: &RuntimeState,
    reading: &ClockReading,
    scope: &UsageScope,
) -> bool {
    let Some(finished) = runtime.last_finished_at else {
        return false;
    };
    let Some(attempt) = runtime.last_attempt.as_ref() else {
        return false;
    };
    attempt.scope == *scope
        && reading
            .now_utc
            .signed_duration_since(finished)
            .to_std()
            .is_ok_and(|age| age < RESULT_FRESH_FOR)
}

fn should_replace_snapshot(
    current: Option<&UsageSnapshot>,
    candidate: &UsageSnapshot,
    current_scope: &UsageScope,
) -> bool {
    let Some(existing) = current else {
        return true;
    };
    if candidate.scope.end_date < existing.scope.end_date {
        return false;
    }
    if existing.scope == *current_scope && candidate.scope != *current_scope {
        return false;
    }
    if existing.scope.time_zone == candidate.scope.time_zone {
        return candidate.scope.end_date >= existing.scope.end_date;
    }
    true
}

fn state_from_runtime(runtime: &RuntimeState, reading: &ClockReading) -> UsageCollectionState {
    UsageCollectionState {
        revision: runtime.revision,
        refreshing: runtime.refreshing,
        snapshot: runtime.snapshot.clone(),
        last_attempt: runtime.last_attempt.clone(),
        freshness: freshness(runtime.snapshot.as_ref(), reading),
    }
}

fn freshness(snapshot: Option<&UsageSnapshot>, reading: &ClockReading) -> FreshnessInfo {
    let (status, reason) = match snapshot {
        None => (FreshnessStatus::Unknown, FreshnessReason::NoSnapshot),
        Some(snapshot) if snapshot.scope.time_zone != reading.time_zone => {
            (FreshnessStatus::Stale, FreshnessReason::TimeZoneChanged)
        }
        Some(snapshot)
            if snapshot.scope.end_date != reading.local_date.format("%Y-%m-%d").to_string() =>
        {
            (FreshnessStatus::Stale, FreshnessReason::DateChanged)
        }
        Some(snapshot) => match DateTime::parse_from_rfc3339(&snapshot.summary.collected_at) {
            Err(_) => (FreshnessStatus::Unknown, FreshnessReason::InvalidTimestamp),
            Ok(collected_at) => {
                let collected_at = collected_at.with_timezone(&Utc);
                let age = reading.now_utc.signed_duration_since(collected_at);
                if age < chrono::Duration::zero() {
                    (FreshnessStatus::Unknown, FreshnessReason::ClockSkew)
                } else if age.to_std().is_ok_and(|value| value > STALE_AFTER) {
                    (FreshnessStatus::Stale, FreshnessReason::Expired)
                } else {
                    (FreshnessStatus::Fresh, FreshnessReason::Current)
                }
            }
        },
    };
    FreshnessInfo {
        status,
        reason,
        checked_at: timestamp(reading.now_utc),
        current_date: reading.local_date.format("%Y-%m-%d").to_string(),
        current_time_zone: reading.time_zone.clone(),
        stale_after_seconds: STALE_AFTER.as_secs() as u32,
    }
}

fn timestamp(value: DateTime<Utc>) -> String {
    // Fixed precision keeps lexical comparison in the TypeScript reducer
    // chronological when two reads share one transition revision.
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

pub(crate) fn system_time_zone() -> String {
    match jiff::tz::TimeZone::try_system() {
        Ok(tz) => tz
            .iana_name()
            .map(str::to_string)
            .unwrap_or_else(|| "system".to_string()),
        Err(_) => "system".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::usage::{CoverageInfo, DailyUsage, TokenBreakdown};

    struct FakeClock(Mutex<ClockReading>);

    impl FakeClock {
        fn new(now: &str, date: &str, zone: &str) -> Self {
            Self(Mutex::new(ClockReading {
                now_utc: DateTime::parse_from_rfc3339(now)
                    .unwrap()
                    .with_timezone(&Utc),
                local_date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
                time_zone: zone.to_string(),
            }))
        }

        fn set(&self, now: &str, date: &str, zone: &str) {
            *self.0.lock().unwrap() = ClockReading {
                now_utc: DateTime::parse_from_rfc3339(now)
                    .unwrap()
                    .with_timezone(&Utc),
                local_date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
                time_zone: zone.to_string(),
            };
        }
    }

    impl StateClock for FakeClock {
        fn read(&self) -> ClockReading {
            self.0.lock().unwrap().clone()
        }
    }

    struct FakeCollector {
        calls: AtomicUsize,
        results: Mutex<VecDeque<Result<UsageSummary, AppError>>>,
    }

    impl FakeCollector {
        fn new(results: Vec<Result<UsageSummary, AppError>>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                results: Mutex::new(results.into()),
            }
        }
    }

    impl UsageCollector for FakeCollector {
        fn collect(&self, _scope: &UsageScope) -> Result<UsageSummary, AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.results.lock().unwrap().pop_front().unwrap()
        }
    }

    fn summary(date: &str, collected_at: &str, total: u64) -> UsageSummary {
        let day = DailyUsage {
            date: date.to_string(),
            total_tokens: total,
            token_breakdown: TokenBreakdown {
                input_tokens: total,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                reasoning_tokens: 0,
                unclassified_tokens: 0,
            },
            estimated_cost_usd: None,
            cost_unknown_reason: None,
            cache_read_share: None,
            agents: vec![],
        };
        UsageSummary {
            collected_at: collected_at.to_string(),
            today: day.clone(),
            last7_days: vec![day],
            coverage: CoverageInfo::complete(),
        }
    }

    #[test]
    fn first_failure_is_safe_and_retry_success_recovers() {
        let clock = Arc::new(FakeClock::new(
            "2026-09-06T04:00:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(FakeCollector::new(vec![
            Err(AppError::filesystem("C:\\secret\\record.json".into())),
            Ok(summary("2026-09-06", "2026-09-06T04:00:03Z", 42)),
        ]));
        let coordinator = CollectionCoordinator::with_dependencies(clock.clone(), collector);

        let failed = coordinator.refresh(RefreshTrigger::Startup, |_| {});
        assert!(failed.snapshot.is_none());
        assert_eq!(
            failed.last_attempt.as_ref().unwrap().failure,
            Some(RefreshFailureKind::Failed)
        );
        assert!(!serde_json::to_string(&failed).unwrap().contains("secret"));

        clock.set("2026-09-06T04:00:03Z", "2026-09-06", "Asia/Shanghai");
        let recovered = coordinator.refresh(RefreshTrigger::Manual, |_| {});
        assert_eq!(recovered.snapshot.unwrap().summary.today.total_tokens, 42);
        assert_eq!(recovered.freshness.status, FreshnessStatus::Fresh);
        assert_eq!(
            recovered.last_attempt.unwrap().outcome,
            RefreshOutcome::Succeeded
        );
    }

    #[test]
    fn failure_keeps_recent_success_without_calling_it_stale() {
        let clock = Arc::new(FakeClock::new(
            "2026-09-06T04:00:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(FakeCollector::new(vec![
            Ok(summary("2026-09-06", "2026-09-06T04:00:00Z", 10)),
            Err(AppError::filesystem("hidden".into())),
        ]));
        let coordinator = CollectionCoordinator::with_dependencies(clock.clone(), collector);
        coordinator.refresh(RefreshTrigger::Startup, |_| {});
        clock.set("2026-09-06T04:05:00Z", "2026-09-06", "Asia/Shanghai");
        let failed = coordinator.refresh(RefreshTrigger::Periodic, |_| {});

        assert_eq!(failed.snapshot.unwrap().summary.today.total_tokens, 10);
        assert_eq!(failed.freshness.status, FreshnessStatus::Fresh);
        assert_eq!(failed.last_attempt.unwrap().outcome, RefreshOutcome::Failed);
    }

    #[test]
    fn ten_minute_threshold_and_cross_day_are_distinct() {
        let clock = Arc::new(FakeClock::new(
            "2026-09-06T04:00:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(FakeCollector::new(vec![Ok(summary(
            "2026-09-06",
            "2026-09-06T04:00:00Z",
            10,
        ))]));
        let coordinator = CollectionCoordinator::with_dependencies(clock.clone(), collector);
        coordinator.refresh(RefreshTrigger::Startup, |_| {});

        clock.set("2026-09-06T04:10:00Z", "2026-09-06", "Asia/Shanghai");
        assert_eq!(
            coordinator.current_state().freshness.status,
            FreshnessStatus::Fresh
        );
        clock.set("2026-09-06T04:10:01Z", "2026-09-06", "Asia/Shanghai");
        assert_eq!(
            coordinator.current_state().freshness.reason,
            FreshnessReason::Expired
        );
        clock.set("2026-09-06T16:00:01Z", "2026-09-07", "Asia/Shanghai");
        assert_eq!(
            coordinator.current_state().freshness.reason,
            FreshnessReason::DateChanged
        );
    }

    #[test]
    fn date_change_during_collection_triggers_one_recovery_attempt() {
        struct MidnightCollector {
            clock: Arc<FakeClock>,
            calls: AtomicUsize,
        }
        impl UsageCollector for MidnightCollector {
            fn collect(&self, scope: &UsageScope) -> Result<UsageSummary, AppError> {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    self.clock
                        .set("2026-09-06T16:00:01Z", "2026-09-07", "Asia/Shanghai");
                }
                Ok(summary(
                    &scope.end_date,
                    if call == 0 {
                        "2026-09-06T16:00:00Z"
                    } else {
                        "2026-09-06T16:00:01Z"
                    },
                    call as u64 + 1,
                ))
            }
        }

        let clock = Arc::new(FakeClock::new(
            "2026-09-06T15:59:59Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(MidnightCollector {
            clock: clock.clone(),
            calls: AtomicUsize::new(0),
        });
        let coordinator = CollectionCoordinator::with_dependencies(clock, collector.clone());
        let state = coordinator.refresh(RefreshTrigger::Periodic, |_| {});

        assert_eq!(collector.calls.load(Ordering::SeqCst), 2);
        assert_eq!(state.snapshot.unwrap().scope.end_date, "2026-09-07");
        assert_eq!(
            state.last_attempt.unwrap().trigger,
            RefreshTrigger::Recovery
        );
    }

    #[test]
    fn short_failure_backoff_does_not_relabel_an_attempt_or_success() {
        let clock = Arc::new(FakeClock::new(
            "2026-09-06T04:00:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(FakeCollector::new(vec![Ok(summary(
            "2026-09-06",
            "2026-09-06T04:00:00Z",
            10,
        ))]));
        let coordinator =
            CollectionCoordinator::with_dependencies(clock.clone(), collector.clone());
        let first = coordinator.refresh(RefreshTrigger::Manual, |_| {});
        clock.set("2026-09-06T04:00:01Z", "2026-09-06", "Asia/Shanghai");
        let second = coordinator.refresh(RefreshTrigger::Focus, |_| {});

        assert_eq!(collector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(second.revision, first.revision);
        assert_eq!(
            second.snapshot.unwrap().summary.collected_at,
            "2026-09-06T04:00:00Z"
        );
    }

    #[test]
    fn concurrent_refreshes_share_one_state_attempt() {
        struct BlockingCollector {
            calls: AtomicUsize,
            started: (Mutex<bool>, Condvar),
            released: (Mutex<bool>, Condvar),
        }
        impl UsageCollector for BlockingCollector {
            fn collect(&self, _scope: &UsageScope) -> Result<UsageSummary, AppError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                *self.started.0.lock().unwrap() = true;
                self.started.1.notify_all();
                let mut released = self.released.0.lock().unwrap();
                while !*released {
                    released = self.released.1.wait(released).unwrap();
                }
                Ok(summary("2026-09-06", "2026-09-06T04:00:00Z", 10))
            }
        }

        let clock = Arc::new(FakeClock::new(
            "2026-09-06T04:00:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(BlockingCollector {
            calls: AtomicUsize::new(0),
            started: (Mutex::new(false), Condvar::new()),
            released: (Mutex::new(false), Condvar::new()),
        });
        let coordinator = Arc::new(CollectionCoordinator::with_dependencies(
            clock,
            collector.clone(),
        ));

        let first_coordinator = coordinator.clone();
        let first =
            std::thread::spawn(move || first_coordinator.refresh(RefreshTrigger::Manual, |_| {}));
        let mut started = collector.started.0.lock().unwrap();
        while !*started {
            started = collector.started.1.wait(started).unwrap();
        }
        drop(started);
        let second_coordinator = coordinator.clone();
        let second =
            std::thread::spawn(move || second_coordinator.refresh(RefreshTrigger::Focus, |_| {}));
        *collector.released.0.lock().unwrap() = true;
        collector.released.1.notify_all();

        let first = first.join().unwrap();
        let second = second.join().unwrap();
        assert_eq!(collector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(first.revision, second.revision);
        assert_eq!(
            first.last_attempt.unwrap().id,
            second.last_attempt.unwrap().id
        );
    }

    #[test]
    fn time_zone_change_and_future_timestamp_are_not_fresh() {
        let clock = Arc::new(FakeClock::new(
            "2026-09-06T04:00:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(FakeCollector::new(vec![Ok(summary(
            "2026-09-06",
            "2026-09-06T04:00:00Z",
            10,
        ))]));
        let coordinator = CollectionCoordinator::with_dependencies(clock.clone(), collector);
        coordinator.refresh(RefreshTrigger::Startup, |_| {});

        clock.set("2026-09-06T04:01:00Z", "2026-09-06", "UTC");
        assert_eq!(
            coordinator.current_state().freshness.reason,
            FreshnessReason::TimeZoneChanged
        );
        clock.set("2026-09-06T03:59:59Z", "2026-09-06", "Asia/Shanghai");
        assert_eq!(
            coordinator.current_state().freshness.reason,
            FreshnessReason::ClockSkew
        );
    }

    #[test]
    fn public_state_serializes_only_camel_case_safe_fields() {
        let clock = Arc::new(FakeClock::new(
            "2026-09-06T04:00:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(FakeCollector::new(vec![Ok(summary(
            "2026-09-06",
            "2026-09-06T04:00:00Z",
            10,
        ))]));
        let coordinator = CollectionCoordinator::with_dependencies(clock, collector);
        let value =
            serde_json::to_value(coordinator.refresh(RefreshTrigger::Manual, |_| {})).unwrap();
        assert_eq!(value["revision"], 2);
        assert_eq!(value["lastAttempt"]["outcome"], "succeeded");
        assert_eq!(value["snapshot"]["scope"]["timeZone"], "Asia/Shanghai");
        assert_eq!(value["freshness"]["staleAfterSeconds"], 600);
        assert!(value.get("error").is_none());
    }

    #[test]
    fn older_snapshot_never_replaces_a_newer_success() {
        let current = UsageSnapshot {
            scope: UsageScope {
                start_date: "2026-09-01".into(),
                end_date: "2026-09-07".into(),
                time_zone: "UTC".into(),
            },
            summary: summary("2026-09-07", "2026-09-07T01:00:00Z", 20),
        };
        let candidate = UsageSnapshot {
            scope: UsageScope {
                start_date: "2026-08-31".into(),
                end_date: "2026-09-06".into(),
                time_zone: "Asia/Shanghai".into(),
            },
            summary: summary("2026-09-06", "2026-09-06T16:00:00Z", 10),
        };
        assert!(!should_replace_snapshot(
            Some(&current),
            &candidate,
            &current.scope,
        ));
    }

    // --- T06: cross-restart last-success cache -------------------------------

    /// In-memory snapshot store double recording what was persisted.
    struct MemoryStore {
        contents: Mutex<Option<UsageSnapshot>>,
        save_calls: AtomicUsize,
    }

    impl MemoryStore {
        fn with(contents: Option<UsageSnapshot>) -> Self {
            Self {
                contents: Mutex::new(contents),
                save_calls: AtomicUsize::new(0),
            }
        }
    }

    impl SnapshotStore for MemoryStore {
        fn load(&self) -> Option<UsageSnapshot> {
            self.contents
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }

        fn save(&self, snapshot: &UsageSnapshot) -> std::io::Result<()> {
            self.save_calls.fetch_add(1, Ordering::SeqCst);
            *self
                .contents
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(snapshot.clone());
            Ok(())
        }
    }

    #[test]
    fn restored_snapshot_shows_immediately_with_its_original_time_and_date() {
        let store = Arc::new(MemoryStore::with(Some(summary_scope(
            "2026-09-05",
            "2026-09-05T04:00:00Z",
        ))));
        let clock = Arc::new(FakeClock::new(
            "2026-09-06T04:00:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(FakeCollector::new(vec![Ok(summary(
            "2026-09-06",
            "2026-09-06T04:00:05Z",
            99,
        ))]));
        let coordinator =
            CollectionCoordinator::with_parts(clock.clone(), collector.clone(), store.clone());
        coordinator.restore_from_store();

        // Before any refresh: the restored snapshot is visible under its own
        // date, its original collected time, marked stale — last attempt is
        // still empty because no attempt ran in this process.
        let restored = coordinator.current_state();
        let snapshot = restored.snapshot.as_ref().unwrap();
        assert_eq!(snapshot.summary.today.date, "2026-09-05");
        assert_eq!(snapshot.summary.collected_at, "2026-09-05T04:00:00Z");
        assert_eq!(snapshot.scope.end_date, "2026-09-05");
        assert_eq!(restored.freshness.reason, FreshnessReason::DateChanged);
        assert!(restored.last_attempt.is_none());

        // The first refresh still performs a real collection (the cache must
        // not fake a fresh state), then replaces the cache with the new
        // success and the new success time.
        let refreshed = coordinator.refresh(RefreshTrigger::Startup, |_| {});
        assert_eq!(collector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            refreshed.snapshot.as_ref().unwrap().summary.collected_at,
            "2026-09-06T04:00:05Z"
        );
        assert_eq!(
            refreshed.snapshot.as_ref().unwrap().scope.end_date,
            "2026-09-06"
        );
        assert_eq!(
            store.load().unwrap().summary.collected_at,
            "2026-09-06T04:00:05Z"
        );
    }

    #[test]
    fn failed_refresh_never_overwrites_the_persisted_cache() {
        let store = Arc::new(MemoryStore::with(Some(summary_scope(
            "2026-09-06",
            "2026-09-06T04:00:00Z",
        ))));
        let clock = Arc::new(FakeClock::new(
            "2026-09-06T05:00:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(FakeCollector::new(vec![Err(AppError::filesystem(
            "hidden".into(),
        ))]));
        let coordinator =
            CollectionCoordinator::with_parts(clock.clone(), collector, store.clone());
        coordinator.restore_from_store();

        let failed = coordinator.refresh(RefreshTrigger::Startup, |_| {});
        // The last success is still displayed (stale by now, but untouched).
        assert_eq!(
            failed.snapshot.as_ref().unwrap().summary.collected_at,
            "2026-09-06T04:00:00Z"
        );
        assert_eq!(failed.last_attempt.unwrap().outcome, RefreshOutcome::Failed);
        // The persisted cache was not touched by the failure.
        assert_eq!(
            store.load().unwrap().summary.collected_at,
            "2026-09-06T04:00:00Z"
        );
    }

    #[test]
    fn successful_refresh_persists_exactly_the_replaced_snapshot() {
        let store = Arc::new(MemoryStore::with(None));
        let clock = Arc::new(FakeClock::new(
            "2026-09-06T04:00:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(FakeCollector::new(vec![Ok(summary(
            "2026-09-06",
            "2026-09-06T04:00:02Z",
            7,
        ))]));
        let coordinator =
            CollectionCoordinator::with_parts(clock, collector.clone(), store.clone());

        coordinator.refresh(RefreshTrigger::Startup, |_| {});
        assert_eq!(collector.calls.load(Ordering::SeqCst), 1);
        let persisted = store.load().expect("success must be persisted");
        assert_eq!(persisted.summary.today.total_tokens, 7);
        assert_eq!(persisted.summary.collected_at, "2026-09-06T04:00:02Z");
    }

    #[test]
    fn same_day_restored_cache_never_fakes_a_fresh_skip() {
        // A restored cache collected within the 10-minute window must still
        // lead to exactly one real collection: the restored state has no
        // last-finished time in this process, so `cached_state_is_fresh`
        // cannot short-circuit it.
        let store = Arc::new(MemoryStore::with(Some(summary_scope(
            "2026-09-06",
            "2026-09-06T04:00:00Z",
        ))));
        let clock = Arc::new(FakeClock::new(
            "2026-09-06T04:05:00Z",
            "2026-09-06",
            "Asia/Shanghai",
        ));
        let collector = Arc::new(FakeCollector::new(vec![Ok(summary(
            "2026-09-06",
            "2026-09-06T04:05:02Z",
            11,
        ))]));
        let coordinator = CollectionCoordinator::with_parts(clock, collector.clone(), store);
        coordinator.restore_from_store();

        let state = coordinator.refresh(RefreshTrigger::Startup, |_| {});
        assert_eq!(collector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            state.snapshot.as_ref().unwrap().summary.collected_at,
            "2026-09-06T04:05:02Z"
        );
    }

    fn summary_scope(end_date: &str, collected_at: &str) -> UsageSnapshot {
        let mut snap = UsageSnapshot {
            scope: UsageScope {
                start_date: end_date.to_string(),
                end_date: end_date.to_string(),
                time_zone: "Asia/Shanghai".to_string(),
            },
            summary: summary(end_date, collected_at, 42),
        };
        snap.summary.today.date = end_date.to_string();
        snap.summary.last7_days[0].date = end_date.to_string();
        snap
    }
}
