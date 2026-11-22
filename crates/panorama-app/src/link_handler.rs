//! Makes `panorama://` links open this executable on Windows, so a zip install joins a party from
//! a clicked invite with no installer run.
use std::path::Path;

#[cfg(any(target_os = "windows", test))]
const SCHEME_KEY: &str = r"Software\Classes\panorama";

/// The shell command that opens `exe` with the clicked link as its only argument, behind a marker
/// so a crafted link is never read as a flag.
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\" --open-url \"%1\"", exe.display())
}

/// Registers the running executable for `panorama://` links when the keys are missing or point
/// elsewhere. A failure is logged and never stops startup.
pub fn register() {
    #[cfg(target_os = "windows")]
    {
        let result = std::env::current_exe()
            .map_err(|error| error.to_string())
            .and_then(|exe| {
                registry::ensure(SCHEME_KEY, &command_for(&exe)).map_err(|error| error.to_string())
            });
        match result {
            Ok(true) => crate::debug::log("panorama:// links registered".into()),
            Ok(false) => {}
            Err(error) => crate::debug::log(format!("panorama:// links not registered: {error}")),
        }
    }
}

/// Removes the `panorama://` registration, for the uninstaller.
pub fn unregister() {
    #[cfg(target_os = "windows")]
    registry::remove(SCHEME_KEY);
}

#[cfg(target_os = "windows")]
mod registry {
    use std::iter::once;
    use windows::{
        Win32::{
            Foundation::ERROR_SUCCESS,
            System::Registry::{
                HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegGetValueW, RegSetKeyValueW,
            },
        },
        core::{HSTRING, PCWSTR},
    };

    fn read(subkey: &str, name: Option<&str>) -> Option<String> {
        let subkey = HSTRING::from(subkey);
        let name = name.map(HSTRING::from);
        let subkey = PCWSTR(subkey.as_ptr());
        let name = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
        let mut bytes = 0u32;
        // SAFETY: the key and value names outlive the call, and a null data pointer asks only for
        // the size.
        let sized = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey,
                name,
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut bytes),
            )
        };
        if sized != ERROR_SUCCESS {
            return None;
        }
        let mut buffer = vec![0u16; bytes as usize / 2 + 1];
        let mut capacity = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` holds `capacity` bytes and outlives the call.
        let read = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey,
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut capacity),
            )
        };
        if read != ERROR_SUCCESS {
            return None;
        }
        let len = (capacity as usize / 2).min(buffer.len());
        Some(
            String::from_utf16_lossy(&buffer[..len])
                .trim_end_matches('\0')
                .to_owned(),
        )
    }

    fn write(subkey: &str, name: Option<&str>, value: &str) -> windows::core::Result<()> {
        let subkey = HSTRING::from(subkey);
        let name = name.map(HSTRING::from);
        let name = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
        let data: Vec<u16> = value.encode_utf16().chain(once(0)).collect();
        // SAFETY: `data` holds `len * 2` bytes and outlives the call; the key is created if absent.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                name,
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                (data.len() * 2) as u32,
            )
            .ok()
        }
    }

    /// Points `key` at `command`. Returns whether anything was written. The command goes last, so
    /// a half-written scheme is repaired by the next launch.
    pub fn ensure(key: &str, command: &str) -> windows::core::Result<bool> {
        let open = format!(r"{key}\shell\open\command");
        let current = read(&open, None).is_some_and(|set| set.eq_ignore_ascii_case(command));
        if current && read(key, Some("URL Protocol")).is_some() {
            return Ok(false);
        }
        write(key, None, "URL:Panorama watch party")?;
        write(key, Some("URL Protocol"), "")?;
        write(&open, None, command)?;
        Ok(true)
    }

    pub fn remove(key: &str) {
        use windows::Win32::System::Registry::RegDeleteTreeW;
        let key = HSTRING::from(key);
        // SAFETY: the key name outlives the call.
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) };
    }

    #[cfg(test)]
    pub fn command(key: &str) -> Option<String> {
        read(&format!(r"{key}\shell\open\command"), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_quotes_the_exe_and_passes_the_link() {
        let command = command_for(Path::new(r"C:\Program Files\Panorama\panorama.exe"));
        assert_eq!(
            command,
            r#""C:\Program Files\Panorama\panorama.exe" --open-url "%1""#
        );
        assert!(SCHEME_KEY.ends_with("panorama"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn registering_writes_once_and_repairs_a_moved_exe() {
        let key = format!(r"Software\Classes\panorama-test-{}", std::process::id());
        registry::remove(&key);
        let here = command_for(Path::new(r"C:\a\panorama.exe"));
        let moved = command_for(Path::new(r"C:\b\panorama.exe"));
        assert!(registry::ensure(&key, &here).unwrap());
        assert!(!registry::ensure(&key, &here).unwrap());
        assert_eq!(registry::command(&key).as_deref(), Some(here.as_str()));
        assert!(registry::ensure(&key, &moved).unwrap());
        assert_eq!(registry::command(&key).as_deref(), Some(moved.as_str()));
        let old = r#""C:\b\panorama.exe" "%1""#;
        assert_ne!(old, moved);
        assert!(registry::ensure(&key, old).unwrap());
        assert!(registry::ensure(&key, &moved).unwrap());
        assert_eq!(registry::command(&key).as_deref(), Some(moved.as_str()));
        registry::remove(&key);
        assert_eq!(registry::command(&key), None);
    }
}
