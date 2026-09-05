//! Shared startup support for Synth Tauri applications.

#[cfg(any(target_os = "linux", test))]
use std::ffi::OsStr;

/// Disables NVIDIA explicit sync on Linux Wayland unless an override is present.
///
/// Returns true only when this call sets `__NV_DISABLE_EXPLICIT_SYNC=1`.
/// Other platforms are a no-op. Existing values, including empty values, are preserved.
///
/// # Safety
///
/// On Linux, call only during single-threaded startup, before initializing Tauri,
/// GTK/EGL, or libraries that may spawn threads or read the environment concurrently.
pub unsafe fn apply_nvidia_wayland_workaround() -> bool {
    #[cfg(target_os = "linux")]
    {
        const EXPLICIT_SYNC_VAR: &str = "__NV_DISABLE_EXPLICIT_SYNC";
        let wayland_display = std::env::var_os("WAYLAND_DISPLAY");
        let explicit_sync_setting = std::env::var_os(EXPLICIT_SYNC_VAR);
        let nvidia_driver_present = std::path::Path::new("/proc/driver/nvidia/version").exists();

        if should_disable_nvidia_explicit_sync(
            wayland_display.as_deref(),
            nvidia_driver_present,
            explicit_sync_setting.as_deref(),
        ) {
            // SAFETY: The caller guarantees single-threaded startup.
            unsafe { std::env::set_var(EXPLICIT_SYNC_VAR, "1") };
            return true;
        }
    }

    false
}

#[cfg(any(target_os = "linux", test))]
fn should_disable_nvidia_explicit_sync(
    wayland_display: Option<&OsStr>,
    nvidia_driver_present: bool,
    explicit_sync_setting: Option<&OsStr>,
) -> bool {
    wayland_display.is_some() && nvidia_driver_present && explicit_sync_setting.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_to_nvidia_wayland_without_an_override() {
        assert!(should_disable_nvidia_explicit_sync(
            Some(OsStr::new("wayland-0")),
            true,
            None,
        ));
    }

    #[test]
    fn requires_both_wayland_and_nvidia() {
        for (display, nvidia) in [
            (None, true),
            (Some(OsStr::new("wayland-0")), false),
            (None, false),
        ] {
            assert!(!should_disable_nvidia_explicit_sync(display, nvidia, None));
        }
    }

    #[test]
    fn preserves_existing_overrides() {
        for value in ["0", "1", "", "custom"] {
            assert!(!should_disable_nvidia_explicit_sync(
                Some(OsStr::new("wayland-0")),
                true,
                Some(OsStr::new(value)),
            ));
        }
    }

    #[test]
    fn treats_an_empty_wayland_display_as_present() {
        assert!(should_disable_nvidia_explicit_sync(
            Some(OsStr::new("")),
            true,
            None,
        ));
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_unicode_overrides() {
        use std::os::unix::ffi::OsStrExt;

        assert!(!should_disable_nvidia_explicit_sync(
            Some(OsStr::new("wayland-0")),
            true,
            Some(OsStr::from_bytes(b"\xff")),
        ));
    }

    #[cfg(unix)]
    #[test]
    fn treats_a_non_unicode_wayland_display_as_present() {
        use std::os::unix::ffi::OsStrExt;

        assert!(should_disable_nvidia_explicit_sync(
            Some(OsStr::from_bytes(b"\xff")),
            true,
            None,
        ));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn does_nothing_on_other_platforms() {
        // SAFETY: The non-Linux implementation does not access the environment.
        assert!(!unsafe { apply_nvidia_wayland_workaround() });
    }
}
