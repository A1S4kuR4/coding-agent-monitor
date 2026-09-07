//! Single main-instance guard (T05).
//!
//! A second manual launch must focus the running dashboard instead of
//! starting a second UI/tray/refresh loop. The mechanism is the minimal
//! reliable Windows pair:
//!
//! - a named mutex in the `Local\` (per-session) namespace proves that one
//!   main instance already runs — `CreateMutexW` + `ERROR_ALREADY_EXISTS`;
//! - a named auto-reset event forwards "please show yourself" from the
//!   duplicate launch to the primary, which a small listener thread answers
//!   by showing and focusing the main window.
//!
//! The internal collector worker is never intercepted: `main` checks
//! `collector::worker::is_worker_invocation()` FIRST and worker invocations
//! exit before this module is reached, so a worker and the main instance (or
//! several workers) can always coexist by construction.
//!
//! Handles are deliberately never closed: the OS reclaims them when the
//! process exits, and the mutex/event must outlive every call site anyway.

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
        INFINITE,
    };

    /// Per-session namespace keeps different Windows sessions independent.
    const MUTEX_NAME: &str = "Local\\com.codingagentmonitor.single-instance";
    const EVENT_NAME: &str = "Local\\com.codingagentmonitor.single-instance-activate";
    const WAIT_OBJECT_0: u32 = 0;

    type Handle = windows_sys::Win32::Foundation::HANDLE;

    /// Raw kernel handles are not `Send`; this wrapper is the only place one
    /// crosses a thread boundary (into the activation listener). The handle
    /// is never closed by design (see module docs), so a plain copy is safe.
    #[derive(Clone, Copy)]
    struct EventHandle(Handle);

    unsafe impl Send for EventHandle {}

    static ACTIVATION_EVENT: Mutex<Option<EventHandle>> = Mutex::new(None);

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn signal_activation_event(event_name: &str) {
        let name = wide(event_name);
        unsafe {
            let event = OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr());
            if !event.is_null() {
                SetEvent(event);
                CloseHandle(event);
            }
        }
    }

    /// Claims the single main-instance slot. Returns `true` when this process
    /// is the primary and should continue starting; `false` when another main
    /// instance is running and its activation event has been signalled (the
    /// caller should exit without touching any UI, tray or database state).
    pub fn claim_main_instance() -> bool {
        claim_named(MUTEX_NAME, EVENT_NAME)
    }

    /// Test seam: the same claim against explicit names.
    pub(crate) fn claim_named(mutex_name: &str, event_name: &str) -> bool {
        let mutex = wide(mutex_name);
        let event = wide(event_name);
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, mutex.as_ptr());
            if handle.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
                if !handle.is_null() {
                    CloseHandle(handle);
                }
                // Someone else owns the slot: wake their dashboard and leave.
                signal_activation_event(event_name);
                return false;
            }
            let activation_event = CreateEventW(std::ptr::null(), 0, 0, event.as_ptr());
            if activation_event.is_null() {
                // Without the activation channel a second launch could not
                // reach the dashboard — refuse to run half-featured.
                CloseHandle(handle);
                return false;
            }
            *ACTIVATION_EVENT
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some(EventHandle(activation_event));
            true
        }
    }

    /// Spawns the daemon thread that shows the dashboard whenever a duplicate
    /// launch signals the activation event. The event is auto-reset, so every
    /// signal wakes exactly one iteration.
    pub fn spawn_activation_listener(activate: impl Fn() + Send + 'static) {
        let event = ACTIVATION_EVENT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(event) = event else {
            return;
        };
        let _ = std::thread::Builder::new()
            .name("single-instance-activation".into())
            .spawn(move || {
                // `wait_forever` takes the wrapper by value, so the closure
                // captures the Send wrapper (never the raw pointer field).
                while wait_forever(event) {
                    activate();
                }
            });
    }

    /// Blocks until the activation event is signalled. `false` means the wait
    /// failed for a non-signal reason; the listener thread then stops.
    fn wait_forever(event: EventHandle) -> bool {
        unsafe { WaitForSingleObject(event.0, INFINITE) == WAIT_OBJECT_0 }
    }

    #[cfg(test)]
    pub(crate) mod testing {
        pub fn unique_names() -> (String, String) {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock")
                .as_nanos();
            (
                format!("Local\\cam-single-instance-test-{unique}"),
                format!("Local\\cam-single-instance-test-event-{unique}"),
            )
        }
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
mod imp {
    /// Non-Windows builds have no tray product to guard; behave as primary.
    pub fn claim_main_instance() -> bool {
        true
    }

    pub fn spawn_activation_listener(_activate: impl Fn() + Send + 'static) {}
}

#[cfg(not(windows))]
pub use imp::*;

#[cfg(all(test, windows))]
mod tests {
    use super::imp::{claim_named, testing::unique_names};

    #[test]
    fn a_second_claim_of_the_same_slot_is_a_duplicate() {
        let (mutex_name, event_name) = unique_names();
        assert!(
            claim_named(&mutex_name, &event_name),
            "first claim is primary"
        );
        assert!(
            !claim_named(&mutex_name, &event_name),
            "second claim with the same names is a duplicate"
        );
    }

    #[test]
    fn independent_slots_do_not_conflict() {
        let (first_mutex, first_event) = unique_names();
        let (second_mutex, second_event) = unique_names();
        assert!(claim_named(&first_mutex, &first_event));
        assert!(
            claim_named(&second_mutex, &second_event),
            "a differently named slot must stay claimable (worker coexistence)"
        );
    }
}
