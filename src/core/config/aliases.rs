//! Aliases for option keys.
//!
//! This module loads `aliases.json` (from the user config directory) and provides
//! utilities to resolve many user-facing spellings to a single canonical option key.
//!
//! ## What this is for
//!
//! CLI flags, config keys, and headers often come in multiple spellings such as:
//! `showDir`, `show-dir`, `show_dir`, `SHOWDIR`, etc. This module normalizes those
//! spellings and maps them to one canonical key.
//!
//! ## JSON format
//!
//! `aliases.json` must be a JSON object that maps canonical keys to a list of aliases:
//!
//! ```json
//! {
//!   "showDir": ["showDir", "showdir", "show-dir"],
//!   "weakEtags": ["weakEtags", "weaketags", "weak-etags"]
//! }
//! ```
//!
//! ## Normalization rules
//!
//! Normalization is implemented by [`normalize_key`]:
//! - lowercases (Unicode-aware)
//! - removes `-`, `_`, and whitespace
//!
//! This means `dirOverrides404`, `dir-overrides-404`, and `DIR_OVERRIDES_404` all
//! normalize to `diroverrides404` and resolve to the same canonical key.
//!
//! ## Performance notes
//!
//! For repeated lookups, [`Aliases::resolve`] caches an index (built on first use)
//! so lookups are fast and do not rebuild the map each call.
//!
//! If you need the raw index for bulk operations, use [`Aliases::index`].

use serde::Deserialize;
use std::{collections::HashMap, io::Read, path::Path, sync::OnceLock};

use crate::core::config::default::ensure_default_configs_dir;

/// In-memory representation of `aliases.json`.
///
/// The JSON shape is:
///
/// ```text
/// { "canonicalKey": ["alias1", "alias2", ...], ... }
/// ```
///
/// # Examples
///
/// ```
/// use tinyserve::core::config::aliases::Aliases;
///
/// let json = r#"{ "showDir": ["showDir", "show-dir"] }"#;
/// let aliases = Aliases::from_reader(json.as_bytes()).unwrap();
/// assert!(aliases.map.contains_key("showDir"));
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct Aliases {
    /// canonical key -> aliases
    #[serde(flatten)]
    pub map: HashMap<String, Vec<String>>,

    /// Lazily built: normalize(alias) -> canonical key
    #[serde(skip)]
    idx: OnceLock<HashMap<String, String>>,
}

impl Aliases {
    /// Load aliases from any reader (file, memory, etc.).
    ///
    /// # Errors
    /// Returns an error if the reader cannot be read or the JSON is invalid.
    pub fn from_reader<R: Read>(mut reader: R) -> anyhow::Result<Self> {
        let mut s = String::new();
        reader.read_to_string(&mut s)?;
        let mut parsed: Self = serde_json::from_str(&s)?;
        parsed.idx = OnceLock::new();
        Ok(parsed)
    }

    /// Load aliases from a JSON file at `path`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs;
    /// use tinyserve::core::config::aliases::Aliases;
    ///
    /// let p = std::env::temp_dir().join("tinyserve_aliases_test.json");
    /// fs::write(&p, r#"{ "si": ["si", "index"] }"#).unwrap();
    /// let aliases = Aliases::from_path(&p).unwrap();
    /// fs::remove_file(&p).ok();
    ///
    /// assert!(aliases.map.contains_key("si"));
    /// ```
    pub fn from_path(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let f = std::fs::File::open(path)?;
        Self::from_reader(f)
    }

    /// Load aliases from the default tinyserve config directory.
    ///
    /// This will create the config directory if it does not exist.
    ///
    /// # Errors
    /// Returns an error if the directory cannot be created/determined, or if
    /// `aliases.json` is missing/invalid.
    pub fn from_default_location() -> anyhow::Result<Self> {
        let dir = ensure_default_configs_dir()?;
        Self::from_path(dir.join("aliases.json"))
    }

    /// Get (and lazily build) the fast lookup: normalized alias → canonical key.
    ///
    /// The index includes:
    /// - each canonical key (as an accepted input)
    /// - each alias string
    ///
    /// All keys are stored in normalized form using [`normalize_key`].
    ///
    /// # Collision behavior
    /// If two different canonical keys contain aliases that normalize to the same
    /// value, the later inserted one will overwrite the earlier entry.
    pub fn index(&self) -> &HashMap<String, String> {
        self.idx.get_or_init(|| {
            let mut idx = HashMap::new();

            for (canonical, aliases) in &self.map {
                idx.insert(normalize_key(canonical), canonical.clone());
                for a in aliases {
                    idx.insert(normalize_key(a), canonical.clone());
                }
            }

            idx
        })
    }

    /// Resolve an input key to its canonical key.
    ///
    /// This uses a cached index built on first use, so repeated calls are fast.
    pub fn resolve<'a>(&'a self, key: &str) -> Option<&'a str> {
        let nk = normalize_key(key);
        self.index().get(&nk).map(String::as_str)
    }
}

/// Normalize a key for alias matching.
///
/// Rules:
/// - lowercase (Unicode-aware)
/// - drop '-', '_', and whitespace
pub fn normalize_key(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parameterized::parameterized;
    use std::{
        env, fs,
        io::Cursor,
        path::{Path, PathBuf},
        process,
        sync::{Mutex, MutexGuard, OnceLock},
        time::{SystemTime, UNIX_EPOCH},
    };

    // Tests touch the process-wide home override in core::config::default; serialize to avoid races.
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

    fn write_file(path: &Path, contents: &str) {
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn remove_tree(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).ok();
        }
    }

    fn with_default_config_home<F: FnOnce()>(home: PathBuf, f: F) {
        let _g = serial_guard();
        crate::core::config::default::set_home_dir_override(Some(home));
        f();
        crate::core::config::default::set_home_dir_override(None);
    }

    #[test]
    fn from_reader_parses_and_resolve_works() {
        let json = r#"{ "showDir": ["show-dir", "show_dir"] }"#;
        let aliases = Aliases::from_reader(Cursor::new(json)).unwrap();

        assert!(aliases.map.contains_key("showDir"));
        assert_eq!(aliases.resolve("SHOW_DIR"), Some("showDir"));
        assert_eq!(aliases.resolve("show-dir"), Some("showDir"));
        assert_eq!(aliases.resolve("unknown"), None);
    }

    #[test]
    fn from_reader_invalid_json_errors() {
        let bad = r#"{ "showDir": ["show-dir", ] }"#;
        assert!(Aliases::from_reader(Cursor::new(bad)).is_err());
    }

    #[test]
    fn from_path_reads_file() {
        let dir = unique_temp_dir("tinyserve_aliases_from_path");
        let path = dir.join("aliases.json");

        write_file(&path, r#"{ "si": ["si", "index"] }"#);

        let aliases = Aliases::from_path(&path).unwrap();
        assert_eq!(aliases.resolve("INDEX"), Some("si"));

        remove_tree(&dir);
    }

    #[test]
    fn from_default_location_reads_default_configs_dir() {
        let home = unique_temp_dir("tinyserve_aliases_default_home");
        remove_tree(&home);
        fs::create_dir_all(&home).unwrap();

        with_default_config_home(home.clone(), || {
            let configs = ensure_default_configs_dir().unwrap();
            let aliases_path = configs.join("aliases.json");

            write_file(
                &aliases_path,
                r#"{ "weakEtags": ["weak-etags", "weak_etags"] }"#,
            );

            let aliases = Aliases::from_default_location().unwrap();
            assert_eq!(aliases.resolve("WEAK_ETAGS"), Some("weakEtags"));
        });

        remove_tree(&home);
    }

    #[test]
    fn index_is_cached_and_stable() {
        let json = r#"{ "showDir": ["show-dir"] }"#;
        let aliases = Aliases::from_reader(Cursor::new(json)).unwrap();

        let p1 = aliases.index() as *const HashMap<String, String>;
        let p2 = aliases.index() as *const HashMap<String, String>;
        assert_eq!(p1, p2);

        assert_eq!(aliases.resolve("show-dir"), Some("showDir"));
    }

    #[test]
    fn collision_behavior_last_insert_wins() {
        let json = r#"
        {
          "firstKey":  ["f-o-o"],
          "secondKey": ["foo"]
        }
        "#;
        let aliases = Aliases::from_reader(Cursor::new(json)).unwrap();

        let v = aliases.resolve("f_o o").unwrap();
        let idx = aliases.index();
        assert_eq!(idx.get(&normalize_key("foo")).map(String::as_str), Some(v));
    }

    #[parameterized(
        case = {
            ("dirOverrides404", "diroverrides404"),
            ("dir-overrides-404", "diroverrides404"),
            ("DIR_OVERRIDES_404", "diroverrides404"),
            ("  Dir  _  Overrides - 404  ", "diroverrides404"),
        }
    )]
    fn normalize_key_drops_separators_and_lowercases(case: (&'static str, &'static str)) {
        let (input, expected) = case;
        assert_eq!(normalize_key(input), expected);
    }

    #[parameterized(input = { "showDir", "show-dir", "show_dir", "SHOWDIR", "  show  dir  " })]
    fn resolve_accepts_many_spellings(input: &str) {
        let json = r#"{ "showDir": ["show-dir", "show_dir", "showdir"] }"#;
        let aliases = Aliases::from_reader(Cursor::new(json)).unwrap();
        assert_eq!(aliases.resolve(input), Some("showDir"));
    }

    #[test]
    fn index_contains_canonical_key_itself() {
        let json = r#"{ "showDir": ["show-dir"] }"#;
        let aliases = Aliases::from_reader(Cursor::new(json)).unwrap();

        let idx = aliases.index();
        assert_eq!(
            idx.get(&normalize_key("showDir")).map(String::as_str),
            Some("showDir")
        );
    }
}
