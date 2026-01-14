//! # Default config location and bootstrap
//!
//! tinyserve stores user-editable configuration files in a per-user directory.
//! On first run, it creates a config folder if it does not exist.
//!
//! ## Default config directory
//!
//! The default base directory is the user’s home directory. The config path is:
//!
//! - Unix-like systems: `~/.tinyserve/configs/`
//! - Windows: `%USERPROFILE%\.tinyserve\configs\` (based on the resolved home dir)
//!
//! This module provides small helpers to:
//!
//! - compute the default config dir path ([`default_configs_dir`])
//! - ensure it exists ([`ensure_default_configs_dir`])
//!
//! ## Rustdoc conventions (PEP 257-ish, but for Rust)
//!
//! Rust’s equivalent of Python’s docstring conventions is **Rustdoc**:
//!
//! - `//!` for module/file header docs (overview, formats, examples).
//! - `///` for item docs (what a function does, errors, examples).
//! - Summary sentence first, blank line, then details.
//! - Prefer runnable examples (doc tests) where possible.
//!
//! This module’s doc tests avoid relying on the real user home by testing the
//! *folder creation logic* with an explicit base directory (see
//! [`ensure_configs_dir_in`]).
//!
//! ## Notes
//!
//! - The “default config dir” is derived using the [`directories`] crate,
//!   which handles platform differences correctly.

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};

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
    // `ProjectDirs` gives you an OS-correct base directory (home roaming/local etc).
    // Here, we intentionally choose a simple layout: <home>/.tinyserve/configs
    //
    // If you later want XDG/AppData-style locations, you can switch to:
    // ProjectDirs::config_dir() / etc.
    let home = directories::UserDirs::new()
        .and_then(|u| u.home_dir().to_owned().into())
        .ok_or_else(|| anyhow!("failed to determine user home directory"))?;

    Ok(PathBuf::from(home).join(".tinyserve").join("configs"))
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
    let dir = default_configs_dir()?;
    ensure_configs_dir_in(&dir)?;
    Ok(dir)
}

/// Ensures the given directory exists (creates it if missing).
///
/// This is the testable core primitive used by [`ensure_default_configs_dir`].
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
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create configs directory: {}", dir.display()))
}
