//! # Default config location and bootstrap
//!
//! tinyserve stores user-editable configuration files in a per-user directory.
//! On first run, it creates the config folder if it does not exist.
//!
//! ## Default config directory
//!
//! The default base directory is the user’s home directory. The config path is:
//!
//! - Unix-like systems: `~/.tinyserve/configs/`
//! - Windows: `%USERPROFILE%\.tinyserve\configs\` (based on the resolved home dir)
//!
//! This module provides helpers to:
//! - compute the default config dir path ([`default_configs_dir`])
//! - ensure it exists ([`ensure_default_configs_dir`])
//!
//! ## Notes
//!
//! - The “default config dir” is derived using the [`directories`] crate,
//!   which handles platform differences.

use anyhow::{Context, anyhow};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

/// Returns the default `~/.tinyserve/configs` directory for the current user.
///
/// This function does **not** create the directory; use [`ensure_default_configs_dir`]
/// for that.
///
/// # Errors
///
/// Returns an error if a valid home directory cannot be determined.
///
/// # Examples
///
/// ```no_run
/// use tinyserve::core::config::default::default_configs_dir;
///
/// let dir = default_configs_dir().unwrap();
/// println!("tinyserve configs live at: {}", dir.display());
/// ```
pub fn default_configs_dir() -> anyhow::Result<PathBuf> {
    default_configs_dir_from(user_home_dir())
}

/// Ensures the default `~/.tinyserve/configs` directory exists and returns it.
///
/// If the directory does not exist, it will be created (including parents).
///
/// # Errors
///
/// Returns an error if:
/// - the home directory cannot be determined
/// - the directory cannot be created
///
/// # Examples
///
/// ```no_run
/// use tinyserve::core::config::default::ensure_default_configs_dir;
///
/// let dir = ensure_default_configs_dir().unwrap();
/// assert!(dir.exists());
/// ```
pub fn ensure_default_configs_dir() -> anyhow::Result<PathBuf> {
    ensure_default_configs_dir_from(user_home_dir())
}

/// Overrides the resolved user home directory for config path computation.
///
/// Useful for:
/// - embedding tinyserve in environments without a stable OS home
/// - tests that must not write into the real user home
///
/// The override is process-wide and guarded by a mutex. If `home` is `None`,
/// this module behaves as if no home directory can be determined.
///
/// This does not create any directories; it only affects home-dir resolution.
pub(crate) fn set_home_dir_override(home: Option<PathBuf>) {
    let lock = home_dir_override_lock();
    let mut guard = lock.lock().expect("home override mutex poisoned");
    *guard = home;
}

/// Returns the default configs directory for the provided home directory.
///
/// This is the pure, testable unit used by [`default_configs_dir`].
///
/// # Errors
///
/// Returns an error if `home` is `None`.
pub(crate) fn default_configs_dir_from(home: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let home = home.ok_or_else(|| anyhow!("failed to determine user home directory"))?;
    Ok(default_configs_dir_in(&home))
}

/// Ensures the default configs directory exists for the provided home directory.
///
/// This is the pure, testable unit used by [`ensure_default_configs_dir`].
///
/// # Errors
///
/// Returns an error if `home` is `None`, or if directory creation fails.
pub(crate) fn ensure_default_configs_dir_from(home: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let dir = default_configs_dir_from(home)?;
    ensure_configs_dir_in(&dir)?;
    Ok(dir)
}

/// Ensures the given directory exists (creates it if missing).
///
/// This is the core primitive used by [`ensure_default_configs_dir`].
///
/// # Errors
///
/// Returns an error if the directory cannot be created.
///
/// # Examples
///
/// ```
/// use std::fs;
/// use tinyserve::core::config::default::ensure_configs_dir_in;
///
/// let dir = std::env::temp_dir().join("tinyserve_doc_test_configs_dir");
/// if dir.exists() { fs::remove_dir_all(&dir).ok(); }
///
/// ensure_configs_dir_in(&dir).unwrap();
/// assert!(dir.exists());
///
/// fs::remove_dir_all(&dir).ok();
/// ```
pub fn ensure_configs_dir_in(dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create configs directory: {}", dir.display()))
}

/// Resolves the user home directory.
///
/// Resolution order:
/// - override set via [`set_home_dir_override`], if present
/// - OS-resolved home directory via [`directories::UserDirs`]
fn user_home_dir() -> Option<PathBuf> {
    let lock = home_dir_override_lock();
    let guard = lock.lock().expect("home override mutex poisoned");
    if let Some(p) = guard.as_ref() {
        return Some(p.clone());
    }
    drop(guard);

    directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
}

/// Computes the default configs directory under a given home directory.
///
/// The layout is `<home>/.tinyserve/configs`.
fn default_configs_dir_in(home: &Path) -> PathBuf {
    home.join(".tinyserve").join("configs")
}

/// Returns the global home-dir override lock.
fn home_dir_override_lock() -> &'static Mutex<Option<PathBuf>> {
    static HOME_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    HOME_OVERRIDE.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use parameterized::parameterized;
    use std::{
        env, process,
        sync::MutexGuard,
        time::{SystemTime, UNIX_EPOCH},
    };

    static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

    fn serial_guard() -> MutexGuard<'static, ()> {
        SERIAL.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!("{prefix}_{}_{}", process::id(), nanos))
    }

    fn with_home_override<F: FnOnce()>(home: Option<PathBuf>, f: F) {
        let _g = serial_guard();
        set_home_dir_override(home);
        f();
        set_home_dir_override(None);
    }

    #[test]
    fn default_configs_dir_in_builds_expected_path() {
        let base = PathBuf::from("/tmp/somehome");
        let got = default_configs_dir_in(&base);
        assert_eq!(
            got,
            PathBuf::from("/tmp/somehome")
                .join(".tinyserve")
                .join("configs")
        );
    }

    #[test]
    fn ensure_configs_dir_in_creates_and_is_idempotent() {
        let dir = unique_temp_dir("tinyserve_test_configs_dir");
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }

        ensure_configs_dir_in(&dir).unwrap();
        assert!(dir.is_dir());

        ensure_configs_dir_in(&dir).unwrap();
        assert!(dir.is_dir());

        fs::remove_dir_all(&dir).ok();
    }

    type FnUnderTest = fn(Option<PathBuf>) -> anyhow::Result<PathBuf>;

    #[parameterized(
        case = {
            ("default_configs_dir_from", default_configs_dir_from as FnUnderTest),
            ("ensure_default_configs_dir_from", ensure_default_configs_dir_from as FnUnderTest),
        }
    )]
    fn missing_home_returns_error(case: (&'static str, FnUnderTest)) {
        let (name, f) = case;
        let err = f(None).unwrap_err().to_string();
        assert!(
            err.contains("failed to determine user home directory"),
            "{name} unexpected error: {err}"
        );
    }

    #[parameterized(nested = { false, true })]
    fn public_entrypoints_use_override_and_create_dirs(nested: bool) {
        let base = unique_temp_dir("tinyserve_test_home");
        let home = if nested { base.join("nested") } else { base };

        if home.exists() {
            fs::remove_dir_all(&home).ok();
        }
        fs::create_dir_all(&home).unwrap();

        with_home_override(Some(home.clone()), || {
            let expected = default_configs_dir_in(&home);

            let got_default = default_configs_dir().unwrap();
            assert_eq!(got_default, expected);

            let got_ensure = ensure_default_configs_dir().unwrap();
            assert_eq!(got_ensure, expected);
            assert!(got_ensure.is_dir());

            let got_ensure2 = ensure_default_configs_dir().unwrap();
            assert_eq!(got_ensure2, got_ensure);

            let mut it = got_default.components().rev();
            let last = it.next().expect("path should have at least 1 component");
            let second_last = it.next().expect("path should have at least 2 components");
            assert_eq!(last.as_os_str(), "configs");
            assert_eq!(second_last.as_os_str(), ".tinyserve");
        });

        // Remove the whole temp tree (base may be the parent).
        let cleanup_root = if nested {
            home.parent().unwrap_or(Path::new("")).to_path_buf()
        } else {
            home
        };
        if !cleanup_root.as_os_str().is_empty() {
            fs::remove_dir_all(&cleanup_root).ok();
        }
    }

    #[parameterized(use_override = { true, false })]
    fn user_home_dir_uses_override_or_falls_back(use_override: bool) {
        let expected = if use_override {
            let home = unique_temp_dir("tinyserve_test_home_override");
            Some(home)
        } else {
            directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
        };

        let override_value = if use_override { expected.clone() } else { None };

        with_home_override(override_value, || {
            let got = user_home_dir();
            assert_eq!(got, expected);
        });

        // If we created an override home dir path, clean it up (best-effort).
        if use_override {
            if let Some(p) = expected {
                if p.exists() {
                    fs::remove_dir_all(p).ok();
                }
            }
        }
    }
}
