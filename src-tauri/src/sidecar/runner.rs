//! Supervises the ccusage native sidecar and produces the app's `UsageSummary`.
//!
//! This is the only place that constructs the ccusage command line. It runs the
//! released unified daily report (`--by-agent`) plus a temporary focused
//! Antigravity report from a pinned upstream PR. It captures stdout and stderr
//! separately, validates exit status, and enforces a timeout that always kills
//! the child so no process is left behind. Successes are handed to the adapter;
//! nothing here knows the frontend.

use std::{
    collections::HashSet,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::{error::AppError, sidecar::normalize_reports, usage::UsageSummary};

/// Ceiling for each ccusage invocation. Reading local log/database files is
/// normally fast; this is a generous bound that still guarantees a wedged
/// sidecar is killed and never blocks the report forever.
const RUN_TIMEOUT: Duration = Duration::from_secs(15);

/// The released ccusage report remains authoritative for all supported agents.
const UNIFIED_ARGS: &[&str] = &["daily", "--json", "--offline", "--by-agent"];
/// Temporary focused command supplied by the locally pinned Antigravity PR.
const ANTIGRAVITY_ARGS: &[&str] = &["antigravity", "daily", "--json", "--offline"];
/// Label used in error messages; there is no per-agent command any more.
const UNIFIED_LABEL: &str = "the unified daily report";
const ANTIGRAVITY_LABEL: &str = "the Antigravity daily report";
/// How many days of history each report window covers (today-6 .. today).
/// Bounds ccusage's scan so a daily refresh does not re-read years of logs.
const SINCE_DAYS: i64 = 6;

/// Coalesces the tray and dashboard requests that naturally arrive together.
/// Caching failures as well as successes prevents a broken sidecar from being
/// relaunched in a tight loop while still allowing a prompt retry.
const RESULT_FRESH_FOR: Duration = Duration::from_secs(2);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Serializes collection and retains the most recent short-lived result so
/// concurrent callers share one sidecar run instead of merely queuing repeats.
static COLLECT_STATE: Mutex<Option<CachedCollection>> = Mutex::new(None);

/// Filename used by Tauri's `externalBin` after bundling/renaming.
const PACKED_EXE: &str = "ccusage.exe";
const PACKED_ANTIGRAVITY_EXE: &str = "ccusage-antigravity.exe";
/// Fallback for development runs: the staged, target-triple binary. Gated to
/// debug builds so a release EXE never embeds or depends on a dev-machine path.
#[cfg(any(debug_assertions, test))]
const DEV_EXE: &str = "ccusage-x86_64-pc-windows-msvc.exe";
#[cfg(any(debug_assertions, test))]
const DEV_ANTIGRAVITY_EXE: &str = "ccusage-antigravity-x86_64-pc-windows-msvc.exe";

/// Pids of sidecar children still in flight. They are registered right after
/// spawn and removed once the report finishes; the exit path terminates any that
/// remain so no process survives the app.
static ACTIVE_CHILDREN: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
/// Set when the app begins to exit, so a concurrent refresh never starts a new
/// sidecar during teardown.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

fn active_children() -> &'static Mutex<HashSet<u32>> {
    ACTIVE_CHILDREN.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Force-kills `pid` and its entire child tree. Killing the tree matters: a
/// sidecar's grandchildren can hold its stdout/stderr pipe open, and leaving
/// them alive would keep pipe readers blocked forever.
fn kill_tree(pid: u32) {
    let mut command = Command::new("taskkill");
    hide_console_window(&mut command);
    let _ = command
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .output();
}

/// Called on app exit: blocks new sidecars and force-kills any still running so
/// no residual `ccusage` process is left behind.
pub fn begin_shutdown() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
    let pids: Vec<u32> = active_children()
        .lock()
        .expect("active children lock")
        .iter()
        .copied()
        .collect();
    for pid in pids {
        kill_tree(pid);
    }
}

/// Runs the released unified report and the pinned Antigravity compatibility
/// report, then folds both into the app's usage contract. A mutex serializes
/// concurrent callers, and the two child processes run sequentially.
pub fn collect_usage() -> Result<UsageSummary, AppError> {
    let mut state = COLLECT_STATE.lock().expect("collect_usage lock poisoned");
    if let Some(cached) = state.as_ref() {
        if cached.created.elapsed() < RESULT_FRESH_FOR {
            return cached.result.clone();
        }
    }

    let result = collect_usage_uncached();
    *state = Some(CachedCollection {
        created: Instant::now(),
        result: result.clone(),
    });
    result
}

fn collect_usage_uncached() -> Result<UsageSummary, AppError> {
    let (sidecar, antigravity_sidecar) = sidecar_paths()?;
    let now = chrono::Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    // YYYYMMDD start of the window so ccusage does not scan the full history.
    let since = (now - chrono::Duration::days(SINCE_DAYS))
        .format("%Y%m%d")
        .to_string();

    let unified = run_sidecar(UNIFIED_LABEL, &sidecar, UNIFIED_ARGS, &since)?;
    let antigravity = run_sidecar(
        ANTIGRAVITY_LABEL,
        &antigravity_sidecar,
        ANTIGRAVITY_ARGS,
        &since,
    )?;
    // Stamped only after the sidecar succeeds, so a failed collection never
    // advances the UI's "last updated" clock.
    let collected_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    normalize_reports(&unified, &antigravity, &today, &collected_at)
}

/// Finds the ccusage executable. Packaged builds keep it next to the main exe
/// (`ccusage.exe`); development builds fall back to the staged triple binary.
fn sidecar_paths() -> Result<(PathBuf, PathBuf), AppError> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let packed = dir.join(PACKED_EXE);
            let antigravity = dir.join(PACKED_ANTIGRAVITY_EXE);
            if packed.exists() && antigravity.exists() {
                return Ok((packed, antigravity));
            }
        }
    }
    // Development-only fallback: the staged binary under the crate. Excluded
    // from release builds so production never carries a dev-machine path.
    #[cfg(any(debug_assertions, test))]
    {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(DEV_EXE);
        let antigravity = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(DEV_ANTIGRAVITY_EXE);
        if dev.exists() && antigravity.exists() {
            return Ok((dev, antigravity));
        }
    }
    Err(AppError::sidecar_missing())
}

fn run_sidecar(
    label: &str,
    sidecar: &Path,
    base: &[&str],
    since: &str,
) -> Result<String, AppError> {
    if SHUTTING_DOWN.load(Ordering::SeqCst) {
        return Err(AppError::sidecar_failed(
            label,
            "application is shutting down".into(),
        ));
    }
    // Append the window bound (`--since <YYYYMMDD>`) to the static base args so
    // ccusage reads only the last few days instead of the whole log history.
    let args: Vec<String> = base
        .iter()
        .map(|s| s.to_string())
        .chain(["--since".to_string(), since.to_string()])
        .collect();
    let mut command = Command::new(sidecar);
    hide_console_window(&mut command);
    let mut child = command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            AppError::sidecar_failed(label, format!("failed to start sidecar: {error}"))
        })?;

    let pid = child.id();
    active_children()
        .lock()
        .expect("active children lock")
        .insert(pid);

    // Shutdown can begin after the pre-spawn check but before registration.
    // Rechecking after insertion closes that gap: either this path kills the
    // child, or begin_shutdown sees the registered pid in its snapshot.
    if SHUTTING_DOWN.load(Ordering::SeqCst) {
        active_children()
            .lock()
            .expect("active children lock")
            .remove(&pid);
        kill_tree(pid);
        let _ = child.wait();
        return Err(AppError::sidecar_failed(
            label,
            "application is shutting down".into(),
        ));
    }
    let result = await_capture(label, child, RUN_TIMEOUT);
    active_children()
        .lock()
        .expect("active children lock")
        .remove(&pid);

    let captured = result?;

    if !captured.status.success() {
        let code = captured.status.code().unwrap_or(-1);
        let stderr = captured.stderr.trim();
        return Err(AppError::sidecar_failed(
            label,
            format!("exited with status {code}; stderr: {stderr}"),
        ));
    }
    Ok(captured.stdout)
}

/// Waits for `child` until it exits or `timeout` elapses, draining both pipes so
/// a full pipe buffer can never deadlock the sidecar. On timeout (or a poll
/// error) the child is killed and reaped, so no residual process remains.
fn await_capture(agent: &str, mut child: Child, timeout: Duration) -> Result<Captured, AppError> {
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");

    // Drain each pipe on its own thread; otherwise a child that fills one buffer
    // while we block on the other would deadlock.
    let stdout_thread = thread::spawn(move || read_to_string_all(&mut stdout));
    let stderr_thread = thread::spawn(move || read_to_string_all(&mut stderr));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                // Kill the whole tree so no grandchild keeps a pipe open; then
                // join the readers, which now see EOF and return promptly.
                kill_tree(child.id());
                let _ = child.wait(); // reap so Windows has no dangling handle
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(AppError::sidecar_timeout(agent));
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                kill_tree(child.id());
                let _ = child.wait();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(AppError::sidecar_failed(
                    agent,
                    "failed to poll running sidecar".into(),
                ));
            }
        }
    };

    // stdout is the report payload, so a read failure is fatal; stderr is only
    // diagnostics and its absence should never fail a successful run.
    let stdout = match stdout_thread.join() {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            return Err(AppError::sidecar_failed(
                agent,
                format!("read stdout: {error}"),
            ))
        }
        Err(_) => {
            return Err(AppError::sidecar_failed(
                agent,
                "stdout reader panicked".into(),
            ))
        }
    };
    let stderr = match stderr_thread.join() {
        Ok(Ok(output)) => output,
        _ => String::new(),
    };

    Ok(Captured {
        status,
        stdout,
        stderr,
    })
}

fn read_to_string_all(reader: &mut dyn Read) -> io::Result<String> {
    let mut buffer = String::new();
    reader.read_to_string(&mut buffer)?;
    Ok(buffer)
}

struct Captured {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

#[derive(Clone)]
struct CachedCollection {
    created: Instant,
    result: Result<UsageSummary, AppError>,
}

fn hide_console_window(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny wrapper so tests can drive `await_capture` with Windows built-ins.
    fn cmd(args: &[&str]) -> Child {
        Command::new("cmd")
            .args(["/c"])
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn cmd")
    }

    #[test]
    fn reaps_a_child_that_times_out() {
        // `ping -n 40` runs for ~40s; a 500ms bound must kill its whole tree and
        // return well before that, otherwise the timeout is not truly bounded.
        let child = cmd(&["ping", "-n", "40", "127.0.0.1", ">nul"]);
        let started = std::time::Instant::now();
        let result = await_capture("Test", child, Duration::from_millis(500));
        let elapsed = started.elapsed();
        match result {
            Ok(_) => panic!("expected sidecar_timeout but the child exited normally"),
            Err(AppError { code, .. }) if code == "sidecar_timeout" => {}
            Err(other) => panic!("expected sidecar_timeout, got code `{}`", other.code),
        }
        // The grandchild `ping` holds cmd's stdout open; without a tree kill the
        // pipe join would block until ping exits (~40s). Assert the bound held.
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout reap took {elapsed:?}; the tree-kill bound was not enforced"
        );
    }

    #[test]
    fn captures_stdout_and_success_status() {
        let child = cmd(&["echo", "ok"]);
        let captured = await_capture("Test", child, RUN_TIMEOUT).unwrap();
        assert!(captured.status.success());
        assert_eq!(captured.stdout.trim(), "ok");
    }

    #[test]
    fn surfaces_a_non_zero_exit_status() {
        let child = cmd(&["exit", "7"]);
        let captured = await_capture("Test", child, RUN_TIMEOUT).unwrap();
        assert_eq!(captured.status.code(), Some(7));
    }

    /// End-to-end smoke check against the real staged sidecar and local ccusage
    /// logs. Ignored because it depends on a local install (the committed sidecar
    /// plus Claude/Codex log databases); run explicitly with
    /// `cargo test -- --ignored`. Reads only local data, never the network.
    #[test]
    #[ignore = "reads real local ccusage data"]
    fn collects_real_usage_into_a_seven_day_summary() {
        let summary = collect_usage().expect("collect_usage should succeed against local data");
        assert_eq!(summary.last7_days.len(), 7);
        // today is always the last entry in the range.
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(summary.today.date, today);
        // Emit the exact values rendered by the Dashboard so the parity check can
        // compare them against the raw `ccusage` command on the command line.
        println!("PARITY today.total={}", summary.today.total_tokens);
        println!(
            "PARITY today.agents={:?}",
            summary
                .today
                .agents
                .iter()
                .map(|a| format!("{}={}", a.id, a.tokens))
                .collect::<Vec<_>>()
        );
        println!(
            "PARITY today.cost={:?} cacheShare={:?} collectedAt={}",
            summary.today.estimated_cost_usd, summary.today.cache_read_share, summary.collected_at
        );
        println!(
            "PARITY last7={:?}",
            summary
                .last7_days
                .iter()
                .map(|d| d.total_tokens)
                .collect::<Vec<_>>()
        );
    }
}
