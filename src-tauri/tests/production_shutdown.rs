//! Phase 4B: the shutdown race on the public production entry, isolated in its
//! own test binary because the shared shutdown flag is process-global and
//! sticky (the real application never un-exits, so there is no reset).
//!
//! After this test, subsequent scenarios confirm the killed worker left no
//! residual process, and that a later production call is refused only while
//! the flag is set (exercised via the supervisor directly below).

mod common;

use std::time::Duration;

use coding_agent_monitor_lib::collector::supervisor;
use coding_agent_monitor_lib::collector::worker_runner::{
    collect_usage, production_snapshot_request,
};

#[test]
fn shutdown_mid_refresh_cancels_reaps_and_refuses_new_workers() {
    let _lock = common::ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    supervisor::set_worker_exe_override(std::path::PathBuf::from(env!(
        "CARGO_BIN_EXE_coding-agent-monitor"
    )));

    let root = std::env::temp_dir().join(format!(
        "cam-prod4b-shutdown-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("mkdir");
    let mut env_saved: Vec<(String, Option<std::ffi::OsString>)> = Vec::new();
    for key in common::AGENT_ENV_KEYS {
        env_saved.push((key.to_string(), std::env::var_os(key)));
        unsafe { std::env::remove_var(key) };
    }
    for key in ["HOME", "USERPROFILE", "XDG_CONFIG_HOME"] {
        env_saved.push((key.to_string(), std::env::var_os(key)));
        let empty = root.join("empty-home");
        std::fs::create_dir_all(&empty).expect("mkdir home");
        unsafe { std::env::set_var(key, &empty) };
    }
    let marker = root.join("spawn-marker.txt");
    env_saved.push((
        "CAM_TEST_WORKER_SPAWN_MARKER".to_string(),
        std::env::var_os("CAM_TEST_WORKER_SPAWN_MARKER"),
    ));
    unsafe {
        std::env::set_var("CAM_TEST_WORKER_SPAWN_MARKER", &marker);
        std::env::set_var("CAM_TEST_WORKER_SLEEP_MS", "60000");
    }

    // Flip the app-exit flag mid-flight, exactly like RunEvent::ExitRequested.
    let flag_thread = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(1500));
        coding_agent_monitor_lib::shutdown::begin_shutdown();
    });
    let error = collect_usage().expect_err("shutdown must cancel the in-flight refresh");
    flag_thread.join().expect("flag thread");
    assert!(
        error.message.contains("cancel") || error.message.contains("exiting"),
        "shutdown race must surface as cancellation: {error}"
    );

    // Exactly one worker was started; it was killed and reaped.
    let spawns = std::fs::read_to_string(&marker).unwrap_or_default();
    assert_eq!(spawns.lines().count(), 1, "exactly one worker was started");
    for pid_text in spawns.lines() {
        let pid: u32 = pid_text.trim().parse().expect("pid");
        assert!(
            !process_exists(pid),
            "cancelled worker {pid} must be reaped: no residual process"
        );
    }

    // With the flag still set (the app never un-exits), a fresh supervisor
    // call is refused outright instead of starting a new worker.
    let request = production_snapshot_request();
    let refused = supervisor::collect_snapshot_with_options(
        &request,
        &std::sync::atomic::AtomicBool::new(false),
        Duration::from_secs(5),
    )
    .expect_err("no new worker may start after shutdown");
    assert!(
        matches!(
            refused,
            coding_agent_monitor_lib::collector::CollectorError::Cancelled
        ),
        "post-shutdown collection must be refused: {refused}"
    );

    // Restore env for hygiene (process exits anyway).
    for (key, previous) in env_saved.iter().rev() {
        unsafe {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
    std::fs::remove_dir_all(&root).ok();
}

#[cfg(windows)]
fn process_exists(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}")])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
        .unwrap_or(true)
}

#[cfg(not(windows))]
fn process_exists(_pid: u32) -> bool {
    false
}
