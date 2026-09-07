//! Windows "start with Windows" registration (T05).
//!
//! Uses the standard per-user Run key (`HKCU\Software\Microsoft\Windows\
//! CurrentVersion\Run`) — the same non-elevated mechanism the official Tauri
//! autostart plugin manages. No admin rights, no service, no network, and no
//! writes outside the user's own registry hive. The value data is the quoted
//! current executable path; removing the value disables the registration.
//!
//! The subkey is injectable so tests exercise the real registry API against
//! their own throwaway key under `HKCU\Software\CodingAgentMonitorTests\…`
//! and never touch the product's Run entry (see the test module for the
//! cleanup contract).

use std::path::Path;

use crate::error::AppError;

#[cfg(windows)]
mod imp {
    use super::*;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    use crate::error::AppError;

    /// The standard per-user autostart location.
    pub const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    /// The Run value name. Matches the product identity so users can recognize
    /// (and manually remove) the entry.
    const VALUE_NAME: &str = "Coding Agent Monitor";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn registration_error(code: u32) -> AppError {
        // The message carries only the Win32 error code — never a path or a
        // registry dump.
        AppError {
            code: "startup_registration_failed".into(),
            message: format!("Windows startup registration failed (error {code})."),
        }
    }

    /// Read-only presence check (test assertions only; production never
    /// reconciles the registry behind the user's back).
    #[cfg(test)]
    pub fn is_enabled_at(subkey: &str) -> bool {
        use windows_sys::Win32::System::Registry::{RegOpenKeyExW, RegQueryValueExW};

        let subkey = wide(subkey);
        let value_name = wide(VALUE_NAME);
        let mut handle: HKEY = std::ptr::null_mut();
        unsafe {
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut handle,
            ) != ERROR_SUCCESS
            {
                // A missing key is simply "not registered".
                return false;
            }
            let mut size: u32 = 0;
            let present = RegQueryValueExW(
                handle,
                value_name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            ) == ERROR_SUCCESS;
            let _ = RegCloseKey(handle);
            present
        }
    }

    pub fn set_enabled_at(subkey: &str, enable: bool) -> Result<(), AppError> {
        let subkey = wide(subkey);
        let value_name = wide(VALUE_NAME);
        let mut handle: HKEY = std::ptr::null_mut();
        unsafe {
            let opened = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                std::ptr::null(),
                &mut handle,
                std::ptr::null_mut(),
            );
            if opened != ERROR_SUCCESS {
                return Err(registration_error(opened));
            }
        }

        let result = unsafe {
            if enable {
                let exe = std::env::current_exe().map_err(|error| AppError {
                    code: "startup_registration_failed".into(),
                    message: error.to_string(),
                })?;
                set_run_value(&handle, &value_name, &exe)
            } else {
                let deleted = RegDeleteValueW(handle, value_name.as_ptr());
                // Removing an already-absent value is success.
                if deleted == ERROR_SUCCESS || deleted == ERROR_FILE_NOT_FOUND {
                    Ok(())
                } else {
                    Err(registration_error(deleted))
                }
            }
        };
        unsafe {
            let _ = RegCloseKey(handle);
        }
        result
    }

    unsafe fn set_run_value(handle: &HKEY, value_name: &[u16], exe: &Path) -> Result<(), AppError> {
        // The quotes keep the registered command intact when the install path
        // contains spaces.
        let data = wide(&format!("\"{}\"", exe.display()));
        let written = RegSetValueExW(
            *handle,
            value_name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        );
        if written == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(registration_error(written))
        }
    }

    /// Reads the raw Run value back (test assertions only).
    #[cfg(test)]
    pub(crate) fn read_value_at(subkey: &str) -> Option<String> {
        use windows_sys::Win32::System::Registry::{RegOpenKeyExW, RegQueryValueExW};

        let subkey_w = wide(subkey);
        let value_name = wide(VALUE_NAME);
        let mut handle: HKEY = std::ptr::null_mut();
        unsafe {
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey_w.as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut handle,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let mut size: u32 = 0;
            let needed = RegQueryValueExW(
                handle,
                value_name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            );
            let result = if needed == ERROR_SUCCESS && size > 0 && size % 2 == 0 {
                let mut buffer = vec![0u8; size as usize];
                let read = RegQueryValueExW(
                    handle,
                    value_name.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    buffer.as_mut_ptr(),
                    &mut size,
                );
                if read == ERROR_SUCCESS {
                    let units: Vec<u16> = buffer
                        .chunks_exact(2)
                        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                        .collect();
                    String::from_utf16_lossy(&units)
                        .trim_end_matches('\0')
                        .to_string()
                        .into()
                } else {
                    None
                }
            } else {
                None
            };
            let _ = RegCloseKey(handle);
            result
        }
    }

    /// Test-only cleanup: removes a whole throwaway key tree created by a test.
    #[cfg(test)]
    pub(crate) fn delete_tree_at(subkey: &str) {
        use windows_sys::Win32::System::Registry::RegDeleteTreeW;

        let subkey_w = wide(subkey);
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, subkey_w.as_ptr());
        }
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
mod imp {
    pub const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

    #[cfg(test)]
    pub fn is_enabled_at(_subkey: &str) -> bool {
        false
    }

    pub fn set_enabled_at(_subkey: &str, _enable: bool) -> Result<(), crate::error::AppError> {
        Ok(())
    }
}

#[cfg(not(windows))]
pub use imp::*;

/// Registers or unregisters the per-user autostart entry for the running
/// executable.
pub fn set_enabled(enable: bool) -> Result<(), AppError> {
    imp::set_enabled_at(RUN_SUBKEY, enable)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    use std::time::{SystemTime, UNIX_EPOCH};

    /// Unique throwaway subkey per test run. Nothing here touches the product
    /// Run entry; each test deletes its own key tree when done.
    fn test_subkey() -> String {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        format!("Software\\CodingAgentMonitorTests\\{unique}")
    }

    #[test]
    fn set_enabled_writes_and_removes_the_run_value() {
        let subkey = test_subkey();
        // Starts absent.
        assert!(!is_enabled_at(&subkey));

        set_enabled_at(&subkey, true).expect("enable autostart in test subkey");
        assert!(is_enabled_at(&subkey));

        // The stored command is the quoted current executable path.
        let stored = imp::read_value_at(&subkey).expect("run value present");
        let exe = std::env::current_exe().expect("current exe");
        assert_eq!(stored, format!("\"{}\"", exe.display()));

        // Re-registering is idempotent, not an error.
        set_enabled_at(&subkey, true).expect("re-register");
        assert!(is_enabled_at(&subkey));

        set_enabled_at(&subkey, false).expect("disable autostart in test subkey");
        assert!(!is_enabled_at(&subkey));
        // Removing an absent value again stays a success.
        set_enabled_at(&subkey, false).expect("disable again");

        // Cleanup contract: the throwaway test key tree is removed and its
        // absence is asserted, so the user's registry is left untouched.
        imp::delete_tree_at(&subkey);
        assert!(!is_enabled_at(&subkey));
    }
}
